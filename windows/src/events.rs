//! Platform-neutral event types. The Windows layer turns wlanapi
//! notifications and Event Log records into these and the monitor consumes
//! them, so the monitor can be tested on any host.

use crate::Date;

/// wlanapi ACM and MSM notifications that matter to the monitor.
#[derive(Clone, Debug, PartialEq)]
pub enum WlanEvent {
    ConnectionStart,
    ConnectionComplete,
    ConnectionAttemptFail {
        reason: u32,
        reason_text: String,
    },
    Disconnecting,
    /// ACM disconnected. `reason` is a WLAN_REASON_CODE from wlanapi. That
    /// namespace differs from the 802.11 reason codes in a deauth frame.
    Disconnected {
        reason: u32,
        reason_text: String,
    },
    Associating,
    Associated {
        bssid: Option<String>,
    },
    Authenticating,
    Connected {
        bssid: Option<String>,
    },
    RoamingStart,
    RoamingEnd,
    Disassociating,
    MsmDisconnected {
        bssid: Option<String>,
        reason: u32,
        reason_text: String,
    },
    SignalQualityChange(u32),
    RadioStateChange,
    Other(String),
}

impl WlanEvent {
    pub fn settle_reason(&self) -> Option<&'static str> {
        match self {
            WlanEvent::ConnectionComplete => Some("connection-complete"),
            WlanEvent::Disconnected { .. } => Some("disconnected"),
            WlanEvent::Associated { .. } => Some("associated"),
            WlanEvent::Connected { .. } => Some("connected"),
            WlanEvent::RoamingEnd => Some("roaming-end"),
            WlanEvent::MsmDisconnected { .. } => Some("msm-disconnected"),
            WlanEvent::RadioStateChange => Some("radio-state"),
            _ => None,
        }
    }
}

/// A record from one of the subscribed Event Log channels, already
/// reduced to the fields the monitor prints.
#[derive(Clone, Debug, PartialEq)]
pub enum LogEvent {
    /// 802.11 deauthentication or disassociation with its frame reason code.
    Deauth {
        kind: String,
        source: Option<String>,
        reason: u32,
    },
    Dhcp,
    Dhcpv6,
    /// A WLAN-AutoConfig line worth printing without `-v`.
    Line {
        text: String,
        severity: Severity,
    },
    /// Printed only with `-v`.
    Verbose(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MonitorInput {
    Wlan(WlanEvent, Date),
    Log(LogEvent, Date),
    Tick(Date),
    Stop,
}
