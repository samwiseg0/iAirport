//! wlanapi access: interfaces, current connection, BSS list, statistics and
//! notifications.

use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc::Sender;

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_STATE, ERROR_SUCCESS, HANDLE,
};
use windows_sys::Win32::NetworkManagement::WiFi::*;

use super::util::{from_wide, guid_string};
use crate::events::{MonitorInput, WlanEvent};
use crate::format::mac;
use crate::ie;
use crate::link::{BssidSource, LinkSample, LinkState};
use crate::quality::{self, ConnectionQuality};
use crate::Date;

pub struct WlanClient {
    handle: HANDLE,
    notification_context: Option<Box<Sender<MonitorInput>>>,
}

#[derive(Clone)]
pub struct InterfaceInfo {
    pub guid: GUID,
    pub guid_string: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub state: i32,
    pub ssid: String,
    pub bssid: [u8; 6],
    pub phy_type: i32,
    pub signal_quality: u32,
    pub tx_rate_kbps: u32,
    pub auth: i32,
    pub cipher: i32,
}

#[derive(Clone, Debug)]
pub struct BssEntry {
    pub bssid: [u8; 6],
    pub rssi: i32,
    pub center_khz: u32,
    pub phy_type: i32,
    pub ies: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub retry: u64,
    pub failed: u64,
    pub duplicate: u64,
}

impl WlanClient {
    pub fn open() -> Result<WlanClient, u32> {
        let mut negotiated = 0u32;
        let mut handle: HANDLE = ptr::null_mut();
        // SAFETY: both out-pointers are valid for the call.
        let code = unsafe {
            WlanOpenHandle(
                WLAN_API_VERSION_2_0,
                ptr::null(),
                &mut negotiated,
                &mut handle,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(code);
        }
        Ok(WlanClient {
            handle,
            notification_context: None,
        })
    }

    pub fn interfaces(&self) -> Result<Vec<InterfaceInfo>, u32> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
        // SAFETY: handle is open; the list is freed below.
        let code = unsafe { WlanEnumInterfaces(self.handle, ptr::null(), &mut list) };
        if code != ERROR_SUCCESS || list.is_null() {
            return Err(code);
        }
        let mut out = Vec::new();
        // SAFETY: wlanapi places dwNumberOfItems entries after the header.
        unsafe {
            let count = (*list).dwNumberOfItems as usize;
            let first = (*list).InterfaceInfo.as_ptr();
            for i in 0..count {
                let info = &*first.add(i);
                out.push(InterfaceInfo {
                    guid: info.InterfaceGuid,
                    guid_string: guid_string(&info.InterfaceGuid),
                    description: from_wide(&info.strInterfaceDescription),
                });
            }
            WlanFreeMemory(list as *const c_void);
        }
        Ok(out)
    }

    fn query(&self, guid: &GUID, opcode: i32) -> Result<QueryBuffer, u32> {
        let mut size = 0u32;
        let mut data: *mut c_void = ptr::null_mut();
        // SAFETY: handle and guid are valid; QueryBuffer frees the result.
        let code = unsafe {
            WlanQueryInterface(
                self.handle,
                guid,
                opcode,
                ptr::null(),
                &mut size,
                &mut data,
                ptr::null_mut(),
            )
        };
        if code != ERROR_SUCCESS || data.is_null() {
            return Err(code);
        }
        Ok(QueryBuffer { data, size })
    }

    pub fn radio_on(&self, guid: &GUID) -> Option<bool> {
        let buffer = self.query(guid, wlan_intf_opcode_radio_state).ok()?;
        let header = std::mem::size_of::<u32>();
        if (buffer.size as usize) < header {
            return None;
        }
        // SAFETY: the header fits; each PHY entry is checked against the
        // returned size before it is read.
        unsafe {
            let state = buffer.data as *const WLAN_RADIO_STATE;
            let count = ((*state).dwNumberOfPhys as usize).min(64);
            let entry = std::mem::size_of::<WLAN_PHY_RADIO_STATE>();
            let mut any = false;
            let mut any_on = false;
            for i in 0..count {
                if header + (i + 1) * entry > buffer.size as usize {
                    break;
                }
                let phy = ptr::addr_of!((*state).PhyRadioState[i]).read_unaligned();
                any = true;
                if phy.dot11SoftwareRadioState == dot11_radio_state_on
                    && phy.dot11HardwareRadioState == dot11_radio_state_on
                {
                    any_on = true;
                }
            }
            any.then_some(any_on)
        }
    }

    pub fn current_connection(&self, guid: &GUID) -> Result<Option<ConnectionInfo>, u32> {
        let buffer = match self.query(guid, wlan_intf_opcode_current_connection) {
            Ok(b) => b,
            Err(ERROR_INVALID_STATE) => return Ok(None),
            Err(code) => return Err(code),
        };
        let Some(attrs) = buffer.read::<WLAN_CONNECTION_ATTRIBUTES>() else {
            return Ok(None);
        };
        let assoc = &attrs.wlanAssociationAttributes;
        let len = (assoc.dot11Ssid.uSSIDLength as usize).min(32);
        let ssid = String::from_utf8_lossy(&assoc.dot11Ssid.ucSSID[..len]).into_owned();
        Ok(Some(ConnectionInfo {
            state: attrs.isState,
            ssid,
            bssid: assoc.dot11Bssid,
            phy_type: assoc.dot11PhyType,
            signal_quality: assoc.wlanSignalQuality,
            tx_rate_kbps: assoc.ulTxRate,
            auth: attrs.wlanSecurityAttributes.dot11AuthAlgorithm,
            cipher: attrs.wlanSecurityAttributes.dot11CipherAlgorithm,
        }))
    }

    /// Not gated by Location, so it still answers when the connection query
    /// returns ERROR_ACCESS_DENIED.
    pub fn interface_connected(&self, guid: &GUID) -> Option<bool> {
        let buffer = self.query(guid, wlan_intf_opcode_interface_state).ok()?;
        buffer
            .read::<i32>()
            .map(|state| state == wlan_interface_state_connected)
    }

    pub fn channel_number(&self, guid: &GUID) -> Option<u32> {
        let buffer = self.query(guid, wlan_intf_opcode_channel_number).ok()?;
        buffer.read::<u32>().filter(|c| *c > 0)
    }

    /// Many drivers reject this opcode. The BSS list carries the dBm value.
    pub fn rssi(&self, guid: &GUID) -> Option<i32> {
        let buffer = self.query(guid, wlan_intf_opcode_rssi).ok()?;
        buffer.read::<i32>().filter(|r| *r < 0)
    }

    /// Opcode 19, Windows 11 24H2 and later. Older builds and drivers
    /// refuse it, which reads as None.
    pub fn connection_quality(&self, guid: &GUID) -> Option<ConnectionQuality> {
        let buffer = self
            .query(guid, quality::OPCODE_REALTIME_CONNECTION_QUALITY)
            .ok()?;
        // SAFETY: wlanapi returned size bytes at data.
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.data as *const u8, buffer.size as usize) };
        quality::parse(bytes)
    }

    pub fn statistics(&self, guid: &GUID) -> Option<FrameStats> {
        let buffer = self.query(guid, wlan_intf_opcode_statistics).ok()?;
        let entry = std::mem::size_of::<WLAN_PHY_FRAME_STATISTICS>();
        let header = std::mem::size_of::<WLAN_STATISTICS>() - entry;
        if (buffer.size as usize) < header {
            return None;
        }
        // SAFETY: the header fits; each PHY counter block is bounds-checked.
        unsafe {
            let stats = buffer.data as *const WLAN_STATISTICS;
            let count = (*stats).dwNumberOfPhys as usize;
            let first = (*stats).PhyCounters.as_ptr();
            let mut out = FrameStats::default();
            for i in 0..count {
                if header + (i + 1) * entry > buffer.size as usize {
                    break;
                }
                let phy = first.add(i).read_unaligned();
                out.retry = out.retry.wrapping_add(phy.ullRetryCount);
                out.failed = out.failed.wrapping_add(phy.ullFailedCount);
                out.duplicate = out.duplicate.wrapping_add(phy.ullFrameDuplicateCount);
            }
            Some(out)
        }
    }

    /// Reads the cached BSS list without starting a scan. Windows 11 24H2
    /// returns ERROR_ACCESS_DENIED here when Location is off for desktop apps.
    pub fn bss_list(&self, guid: &GUID) -> Result<Vec<BssEntry>, u32> {
        let mut list: *mut WLAN_BSS_LIST = ptr::null_mut();
        // SAFETY: handle and guid are valid; the list is freed below.
        let code = unsafe {
            WlanGetNetworkBssList(
                self.handle,
                guid,
                ptr::null(),
                dot11_BSS_type_any,
                0,
                ptr::null(),
                &mut list,
            )
        };
        if code != ERROR_SUCCESS || list.is_null() {
            return Err(code);
        }
        let mut out = Vec::new();
        // SAFETY: every entry and IE range is checked against dwTotalSize.
        unsafe {
            let total = (*list).dwTotalSize as usize;
            let count = (*list).dwNumberOfItems as usize;
            let base = list as *const u8;
            let entry_size = std::mem::size_of::<WLAN_BSS_ENTRY>();
            let header = std::mem::size_of::<WLAN_BSS_LIST>() - entry_size;
            for i in 0..count {
                let entry_offset = header + i * entry_size;
                if entry_offset + entry_size > total {
                    break;
                }
                let entry = (base.add(entry_offset) as *const WLAN_BSS_ENTRY).read_unaligned();
                let ie_start = entry_offset + entry.ulIeOffset as usize;
                let ie_len = entry.ulIeSize as usize;
                let ies = if ie_len > 0 && ie_start + ie_len <= total {
                    std::slice::from_raw_parts(base.add(ie_start), ie_len).to_vec()
                } else {
                    Vec::new()
                };
                out.push(BssEntry {
                    bssid: entry.dot11Bssid,
                    rssi: entry.lRssi,
                    center_khz: entry.ulChCenterFrequency,
                    phy_type: entry.dot11BssPhyType,
                    ies,
                });
            }
            WlanFreeMemory(list as *const c_void);
        }
        Ok(out)
    }

    pub fn register_notifications(&mut self, sender: Sender<MonitorInput>) -> Result<(), u32> {
        let context = Box::new(sender);
        let raw = &*context as *const Sender<MonitorInput> as *const c_void;
        // SAFETY: the boxed sender lives in self until unregistration.
        let code = unsafe {
            WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_ACM | WLAN_NOTIFICATION_SOURCE_MSM,
                0,
                Some(notification_callback),
                raw,
                ptr::null(),
                ptr::null_mut(),
            )
        };
        // MSM notifications can be refused without Location; ACM alone still
        // carries connect and disconnect.
        let code = if code == ERROR_ACCESS_DENIED {
            // SAFETY: same context pointer and lifetime as above.
            unsafe {
                WlanRegisterNotification(
                    self.handle,
                    WLAN_NOTIFICATION_SOURCE_ACM,
                    0,
                    Some(notification_callback),
                    raw,
                    ptr::null(),
                    ptr::null_mut(),
                )
            }
        } else {
            code
        };
        if code != ERROR_SUCCESS {
            return Err(code);
        }
        self.notification_context = Some(context);
        Ok(())
    }

    fn unregister_notifications(&mut self) {
        if self.notification_context.is_none() {
            return;
        }
        // SAFETY: source NONE removes the callback. wlanapi waits for any
        // running callback before returning, so the box can drop after this.
        unsafe {
            WlanRegisterNotification(
                self.handle,
                WLAN_NOTIFICATION_SOURCE_NONE,
                0,
                None,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
            );
        }
        self.notification_context = None;
    }
}

impl Drop for WlanClient {
    fn drop(&mut self) {
        self.unregister_notifications();
        // SAFETY: the handle came from WlanOpenHandle and is closed once.
        unsafe {
            WlanCloseHandle(self.handle, ptr::null());
        }
    }
}

/// Folds opcode 19 into the sample. Blank beats a wrong value, so only the
/// fields still unknown are filled from the single link.
fn apply_quality(link: &mut LinkSample, q: &ConnectionQuality) {
    link.rx_rate_mbps = q.rx_rate_mbps();
    if link.tx_rate_mbps.is_none() {
        link.tx_rate_mbps = q.tx_rate_mbps();
    }
    if link.phy.is_none() {
        link.phy = ie::phy_name(q.phy_type).map(str::to_string);
    }
    if let [only] = q.links.as_slice() {
        if link.rssi_dbm.is_none() {
            link.rssi_dbm = only.rssi_dbm;
        }
        if link.channel.is_none() {
            link.channel = only.channel();
        }
        if link.band.is_none() {
            link.band = only.band().map(str::to_string);
        }
        if link.width_mhz.is_none() {
            link.width_mhz = only.bandwidth_mhz;
        }
    }
    link.mlo_links = q.mlo_links().to_vec();
}

struct QueryBuffer {
    data: *mut c_void,
    size: u32,
}

impl QueryBuffer {
    fn read<T: Copy>(&self) -> Option<T> {
        if (self.size as usize) < std::mem::size_of::<T>() {
            return None;
        }
        // SAFETY: the size check above covers a whole T.
        Some(unsafe { ptr::read_unaligned(self.data as *const T) })
    }
}

impl Drop for QueryBuffer {
    fn drop(&mut self) {
        // SAFETY: data came from WlanQueryInterface.
        unsafe { WlanFreeMemory(self.data) };
    }
}

pub fn reason_to_string(code: u32) -> String {
    let mut buffer = [0u16; 512];
    // SAFETY: the buffer length is passed and the API terminates within it.
    let result = unsafe {
        WlanReasonCodeToString(code, buffer.len() as u32, buffer.as_mut_ptr(), ptr::null())
    };
    if result != ERROR_SUCCESS {
        return String::new();
    }
    from_wide(&buffer)
}

unsafe extern "system" fn notification_callback(
    data: *mut L2_NOTIFICATION_DATA,
    context: *mut c_void,
) {
    if data.is_null() || context.is_null() {
        return;
    }
    // SAFETY: context is the sender registered above and data is valid for
    // the duration of the callback.
    let (sender, note) = unsafe { (&*(context as *const Sender<MonitorInput>), &*data) };
    if let Some(event) = translate(note) {
        let _ = sender.send(MonitorInput::Wlan(event, chrono::Local::now()));
    }
}

const WLAN_NOTIFICATION_ACM_CONNECTION_ATTEMPT_FAIL: i32 =
    wlan_notification_acm_connection_attempt_fail;
const WLAN_NOTIFICATION_ACM_CONNECTION_COMPLETE: i32 = wlan_notification_acm_connection_complete;
const WLAN_NOTIFICATION_ACM_CONNECTION_START: i32 = wlan_notification_acm_connection_start;
const WLAN_NOTIFICATION_ACM_DISCONNECTED: i32 = wlan_notification_acm_disconnected;
const WLAN_NOTIFICATION_ACM_DISCONNECTING: i32 = wlan_notification_acm_disconnecting;
const WLAN_NOTIFICATION_MSM_ASSOCIATED: i32 = wlan_notification_msm_associated;
const WLAN_NOTIFICATION_MSM_ASSOCIATING: i32 = wlan_notification_msm_associating;
const WLAN_NOTIFICATION_MSM_AUTHENTICATING: i32 = wlan_notification_msm_authenticating;
const WLAN_NOTIFICATION_MSM_CONNECTED: i32 = wlan_notification_msm_connected;
const WLAN_NOTIFICATION_MSM_DISASSOCIATING: i32 = wlan_notification_msm_disassociating;
const WLAN_NOTIFICATION_MSM_DISCONNECTED: i32 = wlan_notification_msm_disconnected;
const WLAN_NOTIFICATION_MSM_LINK_DEGRADED: i32 = wlan_notification_msm_link_degraded;
const WLAN_NOTIFICATION_MSM_LINK_IMPROVED: i32 = wlan_notification_msm_link_improved;
const WLAN_NOTIFICATION_MSM_RADIO_STATE_CHANGE: i32 = wlan_notification_msm_radio_state_change;
const WLAN_NOTIFICATION_MSM_ROAMING_END: i32 = wlan_notification_msm_roaming_end;
const WLAN_NOTIFICATION_MSM_ROAMING_START: i32 = wlan_notification_msm_roaming_start;
const WLAN_NOTIFICATION_MSM_SIGNAL_QUALITY_CHANGE: i32 =
    wlan_notification_msm_signal_quality_change;

fn translate(note: &L2_NOTIFICATION_DATA) -> Option<WlanEvent> {
    let size = note.dwDataSize as usize;
    match note.NotificationSource {
        WLAN_NOTIFICATION_SOURCE_ACM => {
            let reason = if size >= std::mem::size_of::<WLAN_CONNECTION_NOTIFICATION_DATA>()
                && !note.pData.is_null()
            {
                // SAFETY: the payload is at least one WLAN_CONNECTION_NOTIFICATION_DATA.
                Some(
                    unsafe {
                        (note.pData as *const WLAN_CONNECTION_NOTIFICATION_DATA).read_unaligned()
                    }
                    .wlanReasonCode,
                )
            } else {
                None
            };
            Some(match note.NotificationCode as i32 {
                WLAN_NOTIFICATION_ACM_CONNECTION_START => WlanEvent::ConnectionStart,
                WLAN_NOTIFICATION_ACM_CONNECTION_COMPLETE => WlanEvent::ConnectionComplete,
                WLAN_NOTIFICATION_ACM_CONNECTION_ATTEMPT_FAIL => {
                    let reason = reason.unwrap_or(0);
                    WlanEvent::ConnectionAttemptFail {
                        reason,
                        reason_text: reason_to_string(reason),
                    }
                }
                WLAN_NOTIFICATION_ACM_DISCONNECTING => WlanEvent::Disconnecting,
                WLAN_NOTIFICATION_ACM_DISCONNECTED => {
                    let reason = reason.unwrap_or(0);
                    WlanEvent::Disconnected {
                        reason,
                        reason_text: reason_to_string(reason),
                    }
                }
                _ => return None,
            })
        }
        WLAN_NOTIFICATION_SOURCE_MSM => {
            let msm = if size >= std::mem::size_of::<WLAN_MSM_NOTIFICATION_DATA>()
                && !note.pData.is_null()
            {
                // SAFETY: the payload is at least one WLAN_MSM_NOTIFICATION_DATA.
                Some(unsafe { (note.pData as *const WLAN_MSM_NOTIFICATION_DATA).read_unaligned() })
            } else {
                None
            };
            let bssid = msm.as_ref().and_then(|m| mac::from_bytes(&m.dot11MacAddr));
            Some(match note.NotificationCode as i32 {
                WLAN_NOTIFICATION_MSM_ASSOCIATING => WlanEvent::Associating,
                WLAN_NOTIFICATION_MSM_ASSOCIATED => WlanEvent::Associated { bssid },
                WLAN_NOTIFICATION_MSM_AUTHENTICATING => WlanEvent::Authenticating,
                WLAN_NOTIFICATION_MSM_CONNECTED => WlanEvent::Connected { bssid },
                WLAN_NOTIFICATION_MSM_ROAMING_START => WlanEvent::RoamingStart,
                WLAN_NOTIFICATION_MSM_ROAMING_END => WlanEvent::RoamingEnd,
                WLAN_NOTIFICATION_MSM_DISASSOCIATING => WlanEvent::Disassociating,
                WLAN_NOTIFICATION_MSM_DISCONNECTED => {
                    let reason = msm.as_ref().map(|m| m.wlanReasonCode).unwrap_or(0);
                    WlanEvent::MsmDisconnected {
                        bssid,
                        reason,
                        reason_text: reason_to_string(reason),
                    }
                }
                WLAN_NOTIFICATION_MSM_SIGNAL_QUALITY_CHANGE => {
                    let pct = if size >= 4 && !note.pData.is_null() {
                        // SAFETY: the payload is a ULONG and at least 4 bytes long.
                        unsafe { ptr::read_unaligned(note.pData as *const u32) }
                    } else {
                        0
                    };
                    WlanEvent::SignalQualityChange(pct)
                }
                WLAN_NOTIFICATION_MSM_RADIO_STATE_CHANGE => WlanEvent::RadioStateChange,
                WLAN_NOTIFICATION_MSM_LINK_DEGRADED => WlanEvent::Other("link degraded".into()),
                WLAN_NOTIFICATION_MSM_LINK_IMPROVED => WlanEvent::Other("link improved".into()),
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Builds one `LinkState` per sample. The BSS list is heavier than the
/// connection query, so it is refreshed at most every `BSS_REFRESH_SECS`
/// unless the BSSID changed.
pub struct LinkReader {
    guid: GUID,
    bss_cache: Vec<BssEntry>,
    bss_cached_at: Option<Date>,
    bss_cached_bssid: Option<[u8; 6]>,
    bss_denied: bool,
    last_stats: Option<FrameStats>,
}

const BSS_REFRESH_SECS: i64 = 3;

pub struct LinkRead {
    pub state: LinkState,
    pub bss_denied: bool,
}

impl LinkReader {
    pub fn new(guid: GUID) -> Self {
        LinkReader {
            guid,
            bss_cache: Vec::new(),
            bss_cached_at: None,
            bss_cached_bssid: None,
            bss_denied: false,
            last_stats: None,
        }
    }

    pub fn read(&mut self, client: &WlanClient, now: Date) -> LinkRead {
        if client.radio_on(&self.guid) == Some(false) {
            self.last_stats = None;
            return LinkRead {
                state: LinkState::off(),
                bss_denied: false,
            };
        }
        let connection = match client.current_connection(&self.guid) {
            Ok(Some(c)) if c.state == wlan_interface_state_connected => c,
            // Windows 11 24H2 withholds the connection query too when
            // Location is off. The interface state still says connected.
            Err(ERROR_ACCESS_DENIED) => {
                self.last_stats = None;
                self.bss_denied = true;
                let state = if client.interface_connected(&self.guid) == Some(true) {
                    let mut link = LinkSample {
                        bssid_source: Some(BssidSource::Connection),
                        rssi_dbm: client.rssi(&self.guid),
                        channel: client.channel_number(&self.guid),
                        ..Default::default()
                    };
                    if let Some(q) = client.connection_quality(&self.guid) {
                        apply_quality(&mut link, &q);
                    }
                    LinkState::associated(link)
                } else {
                    LinkState::disconnected()
                };
                return LinkRead {
                    state,
                    bss_denied: true,
                };
            }
            _ => {
                self.last_stats = None;
                return LinkRead {
                    state: LinkState::disconnected(),
                    bss_denied: false,
                };
            }
        };
        let mut link = LinkSample {
            ssid: Some(connection.ssid.clone()),
            bssid: mac::from_bytes(&connection.bssid),
            bssid_source: Some(BssidSource::Connection),
            phy: ie::phy_name(connection.phy_type).map(str::to_string),
            tx_rate_mbps: Some(connection.tx_rate_kbps as f64 / 1000.0).filter(|r| *r > 0.0),
            signal_pct: Some(connection.signal_quality.min(100)),
            security: ie::security_from_auth(connection.auth, connection.cipher).map(|s| {
                ie::SecurityInfo {
                    security: Some(s.to_string()),
                    ..Default::default()
                }
            }),
            ..Default::default()
        };

        self.refresh_bss(client, &connection.bssid, now);
        if let Some(entry) = self.bss_cache.iter().find(|e| e.bssid == connection.bssid) {
            link.bssid_source = Some(BssidSource::Bss);
            link.rssi_dbm = Some(entry.rssi).filter(|r| *r < 0);
            link.channel = ie::channel_from_khz(entry.center_khz);
            link.band = ie::band_from_khz(entry.center_khz).map(str::to_string);
            let elements = ie::parse(&entry.ies);
            link.ap_name = ie::ap_name::from_elements(&elements);
            link.width_mhz = ie::channel_width(&elements);
            let security = ie::security(&elements);
            if security.security.is_some() {
                link.security = Some(security);
            }
            if link.phy.is_none() {
                link.phy = ie::phy_name(entry.phy_type).map(str::to_string);
            }
        }
        if link.rssi_dbm.is_none() {
            link.rssi_dbm = client.rssi(&self.guid);
        }
        if link.channel.is_none() {
            link.channel = client.channel_number(&self.guid);
            link.band = link
                .channel
                .and_then(ie::band_from_channel)
                .map(str::to_string);
        }
        if let Some(q) = client.connection_quality(&self.guid) {
            apply_quality(&mut link, &q);
        }
        if let Some(stats) = client.statistics(&self.guid) {
            // Drivers reset these counters on reconnect, so a drop reads as 0.
            if let Some(last) = self.last_stats {
                link.tx_retrans = Some(stats.retry.saturating_sub(last.retry));
                link.tx_fail = Some(stats.failed.saturating_sub(last.failed));
                link.rx_retry = Some(stats.duplicate.saturating_sub(last.duplicate));
            }
            self.last_stats = Some(stats);
        }
        LinkRead {
            state: LinkState::associated(link),
            bss_denied: self.bss_denied,
        }
    }

    fn refresh_bss(&mut self, client: &WlanClient, bssid: &[u8; 6], now: Date) {
        let fresh = self
            .bss_cached_at
            .is_some_and(|t| (now - t).num_seconds() < BSS_REFRESH_SECS);
        let same_bssid = self.bss_cached_bssid.as_ref() == Some(bssid);
        if fresh && same_bssid && !self.bss_cache.is_empty() {
            return;
        }
        match client.bss_list(&self.guid) {
            Ok(list) => {
                self.bss_denied = false;
                self.bss_cache = list;
            }
            Err(ERROR_ACCESS_DENIED) => {
                self.bss_denied = true;
                self.bss_cache.clear();
            }
            Err(_) => {}
        }
        self.bss_cached_at = Some(now);
        self.bss_cached_bssid = Some(*bssid);
    }
}
