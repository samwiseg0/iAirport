//! `WLAN_REALTIME_CONNECTION_QUALITY`, the answer to
//! `wlan_intf_opcode_realtime_connection_quality` (opcode 19). The opcode
//! arrived with the Windows 11 24H2 SDK (10.0.26100). windows-sys 0.61 has
//! no binding yet, so the layout is declared here from wlanapi.h and the
//! parser is kept free of Windows types so it can be tested on any host.

use crate::ie;

pub const OPCODE_REALTIME_CONNECTION_QUALITY: i32 = 19;

/// Drivers that predate the opcode leave the bandwidth at 0.
const UNKNOWN_BANDWIDTH: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
struct RateSet {
    length: u32,
    rates: [u16; 126],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinkInfo {
    link_id: u8,
    center_frequency_mhz: u32,
    bandwidth: u32,
    rssi: i32,
    rate_set: RateSet,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    phy_type: i32,
    link_quality: u32,
    rx_rate: u32,
    tx_rate: u32,
    is_mlo_connection: i32,
    num_links: u32,
}

const LINK_SIZE: usize = std::mem::size_of::<LinkInfo>();
const HEADER_SIZE: usize = std::mem::size_of::<Header>();
const MAX_LINKS: usize = 16;

/// One link of the current connection. A Wi-Fi 7 MLO connection has
/// several, everything else has one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QualityLink {
    pub link_id: u8,
    pub center_mhz: u32,
    pub bandwidth_mhz: Option<u32>,
    pub rssi_dbm: Option<i32>,
}

impl QualityLink {
    pub fn channel(&self) -> Option<u32> {
        ie::channel_from_khz(self.center_mhz.checked_mul(1000)?)
    }

    pub fn band(&self) -> Option<&'static str> {
        ie::band_from_khz(self.center_mhz.checked_mul(1000)?)
    }

    /// `5 GHz ch 36/80 -58 dBm`, dropping the parts the driver left blank.
    pub fn display(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(band) = self.band() {
            parts.push(format!("{band} GHz"));
        }
        match (self.channel(), self.bandwidth_mhz) {
            (Some(ch), Some(bw)) => parts.push(format!("ch {ch}/{bw}")),
            (Some(ch), None) => parts.push(format!("ch {ch}")),
            (None, _) => parts.push(format!("{} MHz", self.center_mhz)),
        }
        if let Some(rssi) = self.rssi_dbm {
            parts.push(format!("{rssi} dBm"));
        }
        parts.join(" ")
    }

    /// Compact CSV form: `id:center_mhz/bandwidth/rssi`, blanks kept.
    pub fn csv(&self) -> String {
        format!(
            "{}:{}/{}/{}",
            self.link_id,
            self.center_mhz,
            self.bandwidth_mhz
                .map(|b| b.to_string())
                .unwrap_or_default(),
            self.rssi_dbm.map(|r| r.to_string()).unwrap_or_default()
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionQuality {
    pub phy_type: i32,
    pub rx_rate_kbps: u32,
    pub tx_rate_kbps: u32,
    pub is_mlo: bool,
    pub links: Vec<QualityLink>,
}

impl ConnectionQuality {
    pub fn rx_rate_mbps(&self) -> Option<f64> {
        Some(self.rx_rate_kbps as f64 / 1000.0).filter(|r| *r > 0.0)
    }

    pub fn tx_rate_mbps(&self) -> Option<f64> {
        Some(self.tx_rate_kbps as f64 / 1000.0).filter(|r| *r > 0.0)
    }

    /// The links worth reporting: only an MLO connection has more than the
    /// one the status line already describes.
    pub fn mlo_links(&self) -> &[QualityLink] {
        if self.is_mlo {
            &self.links
        } else {
            &[]
        }
    }
}

/// Parses the buffer wlanapi returns for opcode 19. The link count is
/// trusted only as far as the buffer reaches.
pub fn parse(bytes: &[u8]) -> Option<ConnectionQuality> {
    if bytes.len() < HEADER_SIZE {
        return None;
    }
    // SAFETY: the header fits and each link block is bounds-checked before
    // its unaligned read.
    let head = unsafe { (bytes.as_ptr() as *const Header).read_unaligned() };
    let count = (head.num_links as usize).min(MAX_LINKS);
    let mut links = Vec::with_capacity(count);
    for i in 0..count {
        let offset = HEADER_SIZE + i * LINK_SIZE;
        if offset + LINK_SIZE > bytes.len() {
            break;
        }
        // SAFETY: offset + LINK_SIZE is within bytes.
        let link = unsafe { (bytes.as_ptr().add(offset) as *const LinkInfo).read_unaligned() };
        links.push(QualityLink {
            link_id: link.link_id,
            center_mhz: link.center_frequency_mhz,
            bandwidth_mhz: Some(link.bandwidth).filter(|b| *b != UNKNOWN_BANDWIDTH),
            rssi_dbm: Some(link.rssi).filter(|r| *r < 0),
        });
    }
    Some(ConnectionQuality {
        phy_type: head.phy_type,
        rx_rate_kbps: head.rx_rate,
        tx_rate_kbps: head.tx_rate,
        is_mlo: head.is_mlo_connection != 0,
        links,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32s(buf: &mut Vec<u8>, values: &[u32]) {
        for v in values {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }

    fn link(buf: &mut Vec<u8>, id: u8, mhz: u32, bw: u32, rssi: i32) {
        buf.push(id);
        buf.extend_from_slice(&[0; 3]);
        u32s(buf, &[mhz, bw]);
        buf.extend_from_slice(&rssi.to_le_bytes());
        u32s(buf, &[4]);
        buf.extend_from_slice(&[0; 126 * 2]);
    }

    #[test]
    fn layout_matches_wlanapi_h() {
        assert_eq!(HEADER_SIZE, 24);
        assert_eq!(LINK_SIZE, 272);
    }

    /// Bytes as returned on Windows 11 26H2 (build 26300) with an Intel
    /// 8260 on channel 116: one link, rx and tx 780 Mbps, bandwidth left 0.
    #[test]
    fn parses_single_link_from_live_capture() {
        let mut buf = Vec::new();
        u32s(&mut buf, &[8, 81, 780_000, 780_000, 0, 1]);
        link(&mut buf, 0, 5580, 0, -62);
        assert_eq!(buf.len(), 296);
        let q = parse(&buf).unwrap();
        assert_eq!(q.phy_type, 8);
        assert_eq!(q.rx_rate_mbps(), Some(780.0));
        assert_eq!(q.tx_rate_mbps(), Some(780.0));
        assert!(!q.is_mlo);
        assert_eq!(q.links.len(), 1);
        let l = q.links[0];
        assert_eq!(l.channel(), Some(116));
        assert_eq!(l.band(), Some("5"));
        assert_eq!(l.bandwidth_mhz, None);
        assert_eq!(l.rssi_dbm, Some(-62));
        assert_eq!(l.display(), "5 GHz ch 116 -62 dBm");
        assert_eq!(l.csv(), "0:5580//-62");
        assert!(q.mlo_links().is_empty());
    }

    #[test]
    fn parses_mlo_links() {
        let mut buf = Vec::new();
        u32s(&mut buf, &[10, 95, 2_400_000, 1_900_000, 1, 2]);
        link(&mut buf, 0, 5180, 80, -58);
        link(&mut buf, 1, 6115, 160, -61);
        let q = parse(&buf).unwrap();
        assert!(q.is_mlo);
        assert_eq!(q.mlo_links().len(), 2);
        assert_eq!(q.mlo_links()[0].display(), "5 GHz ch 36/80 -58 dBm");
        assert_eq!(q.mlo_links()[1].display(), "6 GHz ch 33/160 -61 dBm");
        assert_eq!(q.mlo_links()[1].csv(), "1:6115/160/-61");
    }

    #[test]
    fn short_buffers_are_safe() {
        assert!(parse(&[0; 10]).is_none());
        let mut buf = Vec::new();
        u32s(&mut buf, &[8, 81, 0, 0, 1, 3]);
        link(&mut buf, 0, 5180, 80, -58);
        buf.extend_from_slice(&[0; 100]);
        let q = parse(&buf).unwrap();
        assert_eq!(q.links.len(), 1);
        assert_eq!(q.rx_rate_mbps(), None);
    }

    #[test]
    fn link_count_is_capped() {
        let mut buf = Vec::new();
        u32s(&mut buf, &[8, 81, 0, 0, 1, u32::MAX]);
        for i in 0..20u8 {
            link(&mut buf, i, 5180, 80, -58);
        }
        assert_eq!(parse(&buf).unwrap().links.len(), MAX_LINKS);
    }

    #[test]
    fn unknown_frequency_prints_mhz() {
        let l = QualityLink {
            link_id: 2,
            center_mhz: 12345,
            bandwidth_mhz: Some(20),
            rssi_dbm: None,
        };
        assert_eq!(l.display(), "12345 MHz");
    }
}
