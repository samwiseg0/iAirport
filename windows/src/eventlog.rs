//! Minimal scanner for the XML that `EvtRender(EvtRenderEventXml)` returns
//! and a classifier for the WLAN-AutoConfig and DHCP client channels.

use std::collections::BTreeMap;

use crate::events::{LogEvent, Severity};
use crate::format::mac;

pub const WLAN_CHANNEL: &str = "Microsoft-Windows-WLAN-AutoConfig/Operational";
pub const DHCP_CHANNEL: &str = "Microsoft-Windows-Dhcp-Client/Operational";
pub const DHCPV6_CHANNEL: &str = "Microsoft-Windows-Dhcpv6-Client/Operational";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventRecord {
    pub event_id: Option<u32>,
    pub channel: Option<String>,
    pub provider: Option<String>,
    pub system_time: Option<String>,
    pub data: BTreeMap<String, String>,
}

impl EventRecord {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.data
            .get(name)
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
    }
}

pub fn scan(xml: &str) -> EventRecord {
    let mut record = EventRecord {
        event_id: element_text(xml, "EventID").and_then(|t| t.trim().parse().ok()),
        channel: element_text(xml, "Channel").map(unescape),
        provider: attribute_in_tag(xml, "Provider", "Name").map(unescape),
        system_time: attribute_in_tag(xml, "TimeCreated", "SystemTime").map(unescape),
        data: BTreeMap::new(),
    };
    let mut rest = xml;
    while let Some(start) = rest.find("<Data ") {
        let tag_body = &rest[start..];
        let Some(close) = tag_body.find('>') else {
            break;
        };
        let tag = &tag_body[..=close];
        let name = attribute(tag, "Name").map(unescape);
        if tag.ends_with("/>") {
            if let Some(name) = name {
                record.data.insert(name, String::new());
            }
            rest = &tag_body[close + 1..];
            continue;
        }
        let after = &tag_body[close + 1..];
        let Some(end) = after.find("</Data>") else {
            break;
        };
        if let Some(name) = name {
            record.data.insert(name, unescape(&after[..end]));
        }
        rest = &after[end + 7..];
    }
    record
}

fn element_text<'a>(xml: &'a str, element: &str) -> Option<&'a str> {
    let open = format!("<{element}");
    let start = xml.find(&open)?;
    let body = &xml[start + open.len()..];
    let next = body.chars().next()?;
    if next != '>' && next != ' ' {
        return None;
    }
    let close = body.find('>')?;
    if body[..close].ends_with('/') {
        return None;
    }
    let after = &body[close + 1..];
    let end = after.find(&format!("</{element}>"))?;
    Some(&after[..end])
}

fn attribute_in_tag<'a>(xml: &'a str, element: &str, attr: &str) -> Option<&'a str> {
    let open = format!("<{element} ");
    let start = xml.find(&open)?;
    let body = &xml[start..];
    let close = body.find('>')?;
    attribute(&body[..=close], attr)
}

fn attribute<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    for quote in ['\'', '"'] {
        let key = format!(" {attr}={quote}");
        if let Some(pos) = tag.find(&key) {
            let value = &tag[pos + key.len()..];
            let end = value.find(quote)?;
            return Some(&value[..end]);
        }
    }
    None
}

pub fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Turns a record into the event the monitor prints. The channel decides
/// the DHCP cases. For WLAN-AutoConfig the EventID decides. A DEAUTH line
/// needs a numeric 802.11 reason, which only some drivers include in the
/// 8003 reason text. Without one the disconnect prints as a plain line.
pub fn classify(record: &EventRecord) -> LogEvent {
    match record.channel.as_deref() {
        Some(DHCP_CHANNEL) => return LogEvent::Dhcp,
        Some(DHCPV6_CHANNEL) => return LogEvent::Dhcpv6,
        _ => {}
    }
    let ssid = record.get("SSID").or(record.get("ProfileName"));
    let quoted = ssid.map(|s| format!("  \"{s}\"")).unwrap_or_default();
    let bssid = record
        .get("BSSID")
        .and_then(|b| mac::normalize(Some(b)))
        .map(|b| format!("  {b}"))
        .unwrap_or_default();
    let reason = record
        .get("ReasonText")
        .or(record.get("FailureReason"))
        .or(record.get("Reason"))
        .map(|r| format!("  {}", r.trim().trim_end_matches('.')))
        .unwrap_or_default();
    match record.event_id {
        Some(8000) => LogEvent::Verbose(format!("WLAN connection start{quoted}")),
        Some(8001) => LogEvent::Verbose(format!("WLAN connected{quoted}{bssid}")),
        Some(8002) => LogEvent::Line {
            text: format!("WLAN connection failed{quoted}{reason}"),
            severity: Severity::Error,
        },
        Some(8003) => {
            if let Some((kind, code)) = deauth_from_text(record.get("Reason")) {
                return LogEvent::Deauth {
                    kind,
                    source: None,
                    reason: code,
                };
            }
            LogEvent::Line {
                text: format!("WLAN disconnected{quoted}{reason}"),
                severity: Severity::Warning,
            }
        }
        Some(11000) => LogEvent::Verbose(format!("802.11 association start{quoted}{bssid}")),
        Some(11001) => {
            let phy = record
                .get("PHYType")
                .map(|p| format!("  {p}"))
                .unwrap_or_default();
            let auth = record
                .get("AuthenticationAlgorithm")
                .map(|a| format!("  {a}"))
                .unwrap_or_default();
            LogEvent::Verbose(format!("802.11 associated{quoted}{bssid}{phy}{auth}"))
        }
        Some(11002) => LogEvent::Line {
            text: format!("802.11 association failed{quoted}{bssid}{reason}"),
            severity: Severity::Error,
        },
        Some(11004) => LogEvent::Verbose(format!("802.11 security stopped{quoted}{bssid}")),
        Some(11005) => LogEvent::Verbose(format!("802.11 security succeeded{quoted}{bssid}")),
        Some(11006) => LogEvent::Line {
            text: format!("802.11 security failed{quoted}{bssid}{reason}"),
            severity: Severity::Error,
        },
        Some(11010) => LogEvent::Verbose(format!("802.11 security started{quoted}{bssid}")),
        Some(12011) => LogEvent::Verbose(format!("802.1X started{quoted}{bssid}")),
        Some(12012) => LogEvent::Verbose(format!("802.1X succeeded{quoted}{bssid}")),
        Some(12013) => LogEvent::Line {
            text: format!("802.1X failed{quoted}{bssid}{reason}"),
            severity: Severity::Error,
        },
        Some(id) => LogEvent::Verbose(format!("WLAN event {id}{quoted}{reason}")),
        None => LogEvent::Verbose("WLAN event".to_string()),
    }
}

/// Looks for "deauth"/"disassoc" plus a number in a reason string, such as
/// "Received deauthentication from the AP, reason code 3".
fn deauth_from_text(text: Option<&str>) -> Option<(String, u32)> {
    let text = text?;
    let lower = text.to_ascii_lowercase();
    let kind = if lower.contains("deauth") {
        "DEAUTH"
    } else if lower.contains("disassoc") {
        "DISASSOC"
    } else {
        return None;
    };
    let after = lower.find("reason").map(|i| &lower[i..]).unwrap_or(&lower);
    let digits: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let code: u32 = digits.parse().ok()?;
    Some((kind.to_string(), code))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sample XML written by hand in the shape EvtRender produces. These
    // are not captures from a live machine.
    fn wlan(event_id: u32, data: &[(&str, &str)]) -> String {
        let mut body = String::new();
        for (name, value) in data {
            body.push_str(&format!("<Data Name='{name}'>{value}</Data>"));
        }
        format!(
            "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-WLAN-AutoConfig' Guid='{{9580d7dd-0379-4658-9870-d5be7d52d6de}}'/><EventID>{event_id}</EventID><Version>0</Version><Level>4</Level><Task>24010</Task><Opcode>0</Opcode><Keywords>0x4000000000000000</Keywords><TimeCreated SystemTime='2026-03-04T10:11:12.1234567Z'/><EventRecordID>4242</EventRecordID><Correlation/><Execution ProcessID='1234' ThreadID='5678'/><Channel>Microsoft-Windows-WLAN-AutoConfig/Operational</Channel><Computer>LAPTOP</Computer><Security UserID='S-1-5-19'/></System><EventData>{body}</EventData></Event>"
        )
    }

    #[test]
    fn scans_system_and_data() {
        let xml = wlan(
            8003,
            &[
                ("InterfaceGuid", "{1b2c3d4e-0000-0000-0000-000000000001}"),
                ("InterfaceDescription", "Intel(R) Wi-Fi 6 AX201 160MHz"),
                ("ConnectionMode", "Automatic connection with a profile"),
                ("ProfileName", "Corp &amp; Guest"),
                ("SSID", "Corp &amp; Guest"),
                ("BSSType", "Infrastructure"),
                ("Reason", "The network is disconnected by the driver."),
            ],
        );
        let record = scan(&xml);
        assert_eq!(record.event_id, Some(8003));
        assert_eq!(record.channel.as_deref(), Some(WLAN_CHANNEL));
        assert_eq!(
            record.provider.as_deref(),
            Some("Microsoft-Windows-WLAN-AutoConfig")
        );
        assert_eq!(
            record.system_time.as_deref(),
            Some("2026-03-04T10:11:12.1234567Z")
        );
        assert_eq!(record.get("SSID"), Some("Corp & Guest"));
        assert_eq!(
            record.get("Reason"),
            Some("The network is disconnected by the driver.")
        );
        assert_eq!(record.data.len(), 7);
    }

    #[test]
    fn handles_empty_data_and_double_quotes() {
        let xml = "<Event><System><EventID Qualifiers=\"0\">11001</EventID><Channel>Microsoft-Windows-WLAN-AutoConfig/Operational</Channel></System><EventData><Data Name=\"BSSID\">00:0B:86:11:22:01</Data><Data Name=\"PHYType\">802.11ax</Data><Data Name=\"Empty\"/><Data Name=\"AuthenticationAlgorithm\">WPA2-Enterprise</Data></EventData></Event>";
        let record = scan(xml);
        assert_eq!(record.event_id, Some(11001));
        assert_eq!(record.get("BSSID"), Some("00:0B:86:11:22:01"));
        assert_eq!(record.get("Empty"), None);
        assert!(record.data.contains_key("Empty"));
        assert_eq!(
            classify(&record),
            LogEvent::Verbose(
                "802.11 associated  00:0b:86:11:22:01  802.11ax  WPA2-Enterprise".to_string()
            )
        );
    }

    #[test]
    fn tolerates_garbage() {
        let record = scan("<Event><System><EventID>");
        assert_eq!(record.event_id, None);
        let record = scan("not xml at all");
        assert_eq!(record, EventRecord::default());
        let record = scan("<Event><EventData><Data Name='X'>unterminated</EventData></Event>");
        assert!(record.data.is_empty());
    }

    #[test]
    fn classifies_wlan_events() {
        let start = scan(&wlan(8000, &[("SSID", "Corp")]));
        assert_eq!(
            classify(&start),
            LogEvent::Verbose("WLAN connection start  \"Corp\"".into())
        );
        let failed = scan(&wlan(
            8002,
            &[
                ("SSID", "Corp"),
                ("FailureReason", "The specific network is not available."),
            ],
        ));
        assert_eq!(
            classify(&failed),
            LogEvent::Line {
                text: "WLAN connection failed  \"Corp\"  The specific network is not available"
                    .into(),
                severity: Severity::Error
            }
        );
        let disc = scan(&wlan(
            8003,
            &[
                ("SSID", "Corp"),
                ("Reason", "The network is disconnected by the driver."),
            ],
        ));
        assert_eq!(
            classify(&disc),
            LogEvent::Line {
                text: "WLAN disconnected  \"Corp\"  The network is disconnected by the driver"
                    .into(),
                severity: Severity::Warning
            }
        );
        let deauth = scan(&wlan(
            8003,
            &[
                ("SSID", "Corp"),
                (
                    "Reason",
                    "Received deauthentication from the access point, reason code 3.",
                ),
            ],
        ));
        assert_eq!(
            classify(&deauth),
            LogEvent::Deauth {
                kind: "DEAUTH".into(),
                source: None,
                reason: 3
            }
        );
        let disassoc = scan(&wlan(
            8003,
            &[("Reason", "Disassociated by peer (reason 8)")],
        ));
        assert_eq!(
            classify(&disassoc),
            LogEvent::Deauth {
                kind: "DISASSOC".into(),
                source: None,
                reason: 8
            }
        );
        let sec = scan(&wlan(
            11006,
            &[
                ("SSID", "Corp"),
                ("BSSID", "00:0b:86:11:22:01"),
                ("ReasonCode", "0x38001"),
                ("ReasonText", "Explicit EAP failure received"),
            ],
        ));
        assert_eq!(
            classify(&sec),
            LogEvent::Line { text: "802.11 security failed  \"Corp\"  00:0b:86:11:22:01  Explicit EAP failure received".into(), severity: Severity::Error }
        );
        let eap = scan(&wlan(
            12013,
            &[
                ("SSID", "Corp"),
                ("ReasonText", "802.1X authentication failed"),
            ],
        ));
        assert!(matches!(
            classify(&eap),
            LogEvent::Line {
                severity: Severity::Error,
                ..
            }
        ));
        for id in [11000, 11004, 11005, 11010, 12011, 12012] {
            assert!(
                matches!(classify(&scan(&wlan(id, &[]))), LogEvent::Verbose(_)),
                "{id}"
            );
        }
        assert_eq!(
            classify(&scan(&wlan(20019, &[]))),
            LogEvent::Verbose("WLAN event 20019".into())
        );
    }

    #[test]
    fn classifies_dhcp_channels() {
        let v4 = "<Event><System><Provider Name='Microsoft-Windows-Dhcp-Client'/><EventID>50036</EventID><Channel>Microsoft-Windows-Dhcp-Client/Operational</Channel></System><EventData><Data Name='Address'>192.0.2.143</Data></EventData></Event>";
        let v6 = "<Event><System><Provider Name='Microsoft-Windows-Dhcpv6-Client'/><EventID>1002</EventID><Channel>Microsoft-Windows-Dhcpv6-Client/Operational</Channel></System></Event>";
        assert_eq!(classify(&scan(v4)), LogEvent::Dhcp);
        assert_eq!(classify(&scan(v6)), LogEvent::Dhcpv6);
    }
}
