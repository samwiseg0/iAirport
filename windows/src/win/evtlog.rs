//! Event Log subscriptions (wevtapi) for WLAN-AutoConfig and DHCP client
//! operational channels.

use std::ffi::c_void;
use std::ptr;
use std::sync::mpsc::Sender;

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_ACCESS_DENIED, ERROR_EVT_CHANNEL_NOT_FOUND,
};
use windows_sys::Win32::System::EventLog::{
    EvtClose, EvtRender, EvtRenderEventXml, EvtSubscribe, EvtSubscribeActionDeliver,
    EvtSubscribeToFutureEvents, EVT_HANDLE, EVT_SUBSCRIBE_NOTIFY_ACTION,
};

use super::util::to_wide;
use crate::eventlog::{self, DHCPV6_CHANNEL, DHCP_CHANNEL, WLAN_CHANNEL};
use crate::events::MonitorInput;

pub struct Subscriptions {
    handles: Vec<EVT_HANDLE>,
    _context: Box<Sender<MonitorInput>>,
}

impl Subscriptions {
    /// Subscribes to the three channels. Each failure becomes one warning
    /// line; the monitor keeps running without that channel.
    pub fn start(sender: Sender<MonitorInput>) -> (Subscriptions, Vec<String>) {
        let context = Box::new(sender);
        let raw = &*context as *const Sender<MonitorInput> as *const c_void;
        let mut handles = Vec::new();
        let mut warnings = Vec::new();
        for channel in [WLAN_CHANNEL, DHCP_CHANNEL, DHCPV6_CHANNEL] {
            let path = to_wide(channel);
            let query = to_wide("*");
            // SAFETY: the strings outlive the call; the context box lives in
            // the returned struct until EvtClose runs for every handle.
            let handle = unsafe {
                EvtSubscribe(
                    0,
                    ptr::null_mut(),
                    path.as_ptr(),
                    query.as_ptr(),
                    0,
                    raw,
                    Some(callback),
                    EvtSubscribeToFutureEvents,
                )
            };
            if handle == 0 {
                // SAFETY: plain thread-local error read.
                let code = unsafe { GetLastError() };
                let why = match code {
                    ERROR_ACCESS_DENIED => "access denied".to_string(),
                    ERROR_EVT_CHANNEL_NOT_FOUND => "channel not found".to_string(),
                    other => format!("error {other}"),
                };
                warnings.push(format!(
                    "Event log subscription to {channel} failed ({why}). Deauth reasons and DHCP lines from that channel will be missing."
                ));
                continue;
            }
            handles.push(handle);
        }
        (
            Subscriptions {
                handles,
                _context: context,
            },
            warnings,
        )
    }
}

impl Drop for Subscriptions {
    fn drop(&mut self) {
        for handle in self.handles.drain(..) {
            // SAFETY: each handle came from EvtSubscribe and is closed once.
            // EvtClose waits for a running callback before returning.
            unsafe {
                EvtClose(handle);
            }
        }
    }
}

unsafe extern "system" fn callback(
    action: EVT_SUBSCRIBE_NOTIFY_ACTION,
    context: *const c_void,
    event: EVT_HANDLE,
) -> u32 {
    if action != EvtSubscribeActionDeliver || context.is_null() || event == 0 {
        return 0;
    }
    let Some(xml) = render_xml(event) else {
        return 0;
    };
    // SAFETY: context is the sender registered in `start`.
    let sender = unsafe { &*(context as *const Sender<MonitorInput>) };
    let record = eventlog::scan(&xml);
    let _ = sender.send(MonitorInput::Log(
        eventlog::classify(&record),
        chrono::Local::now(),
    ));
    0
}

fn render_xml(event: EVT_HANDLE) -> Option<String> {
    let mut used = 0u32;
    let mut properties = 0u32;
    // SAFETY: a zero-sized buffer is allowed; the API reports the size needed.
    unsafe {
        EvtRender(
            0,
            event,
            EvtRenderEventXml,
            0,
            ptr::null_mut(),
            &mut used,
            &mut properties,
        );
    }
    if used == 0 {
        return None;
    }
    let mut buffer = vec![0u16; used.div_ceil(2) as usize + 1];
    // SAFETY: the buffer holds `used` bytes plus a terminator.
    let ok = unsafe {
        EvtRender(
            0,
            event,
            EvtRenderEventXml,
            (buffer.len() * 2) as u32,
            buffer.as_mut_ptr() as *mut c_void,
            &mut used,
            &mut properties,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(super::util::from_wide(&buffer))
}
