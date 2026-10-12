use crate::ie::SecurityInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioStatus {
    Off,
    Disconnected,
    Associated,
}

impl RadioStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RadioStatus::Off => "off",
            RadioStatus::Disconnected => "disconnected",
            RadioStatus::Associated => "associated",
        }
    }
}

/// Where the BSSID and RSSI came from. `Bss` means the cached BSS list
/// entry for the connected AP was found. `Connection` means only
/// `wlan_intf_opcode_current_connection` answered, which is what happens
/// when Location access is off on Windows 11 24H2 and later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BssidSource {
    Bss,
    Connection,
}

impl BssidSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            BssidSource::Bss => "bss",
            BssidSource::Connection => "connection",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkSample {
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub bssid_source: Option<BssidSource>,
    pub ap_name: Option<String>,
    pub channel: Option<u32>,
    pub width_mhz: Option<u32>,
    pub band: Option<String>,
    pub phy: Option<String>,
    pub security: Option<SecurityInfo>,
    pub tx_rate_mbps: Option<f64>,
    pub rssi_dbm: Option<i32>,
    pub signal_pct: Option<u32>,
    pub tx_retrans: Option<u64>,
    pub tx_fail: Option<u64>,
    pub rx_retry: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkState {
    pub status: RadioStatus,
    pub link: LinkSample,
}

impl LinkState {
    pub fn off() -> Self {
        LinkState {
            status: RadioStatus::Off,
            link: LinkSample::default(),
        }
    }

    pub fn disconnected() -> Self {
        LinkState {
            status: RadioStatus::Disconnected,
            link: LinkSample::default(),
        }
    }

    pub fn associated(link: LinkSample) -> Self {
        LinkState {
            status: RadioStatus::Associated,
            link,
        }
    }
}
