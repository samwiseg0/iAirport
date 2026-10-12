use crate::Date;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextColor {
    Green,
    Red,
    Cyan,
    Magenta,
    Yellow,
    White,
    Dim,
    None,
}

pub fn style(text: &str, color: TextColor, enabled: bool, bold: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    let mut codes: Vec<&str> = Vec::new();
    if bold {
        codes.push("1");
    }
    match color {
        TextColor::Green => codes.push("32"),
        TextColor::Red => codes.push("31"),
        TextColor::Cyan => codes.push("36"),
        TextColor::Magenta => codes.push("35"),
        TextColor::Yellow => codes.push("33"),
        TextColor::White => codes.push("37"),
        TextColor::Dim => codes.push("2"),
        TextColor::None => {}
    }
    if codes.is_empty() {
        return text.to_string();
    }
    format!("\u{001B}[{}m{}\u{001B}[0m", codes.join(";"), text)
}

pub mod time {
    use crate::Date;

    pub fn status(date: &Date) -> String {
        date.format("%Y/%m/%d %H:%M:%S").to_string()
    }

    pub fn csv(date: &Date) -> String {
        date.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string()
    }

    pub fn json(date: &Date) -> String {
        csv(date)
    }

    pub fn epoch_ms(date: &Date) -> String {
        date.timestamp_millis().to_string()
    }

    pub fn seconds_between(later: &Date, earlier: &Date) -> f64 {
        (*later - *earlier).num_milliseconds() as f64 / 1000.0
    }

    pub fn millis_between(later: &Date, earlier: &Date) -> i64 {
        (*later - *earlier).num_milliseconds()
    }
}

pub mod csv {
    pub fn field(value: Option<&str>) -> String {
        let Some(value) = value else {
            return String::new();
        };
        if value.contains(',')
            || value.contains('"')
            || value.contains('\n')
            || value.contains('\r')
        {
            return format!("\"{}\"", value.replace('"', "\"\""));
        }
        value.to_string()
    }

    pub fn row(fields: &[Option<String>]) -> String {
        let mut out = fields
            .iter()
            .map(|f| field(f.as_deref()))
            .collect::<Vec<_>>()
            .join(",");
        out.push('\n');
        out
    }
}

pub mod units {
    pub fn data(bytes: u64) -> String {
        if bytes == 0 {
            return "0".to_string();
        }
        if bytes < 999 {
            return format!("{bytes}B");
        }
        if bytes < 999_999 {
            return format!("{:.0}K", bytes as f64 / 1024.0);
        }
        if bytes < 999_999_999 {
            return format!("{:.1}M", bytes as f64 / 1024.0 / 1024.0);
        }
        format!("{:.1}G", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    }

    pub fn rate(bits_per_second: u64) -> String {
        if bits_per_second == 0 {
            return "0".to_string();
        }
        if bits_per_second < 999 {
            return bits_per_second.to_string();
        }
        if bits_per_second < 999_999 {
            return format!("{:.0}K", bits_per_second as f64 / 1000.0);
        }
        if bits_per_second < 999_999_999 {
            return format!("{:.1}M", bits_per_second as f64 / 1_000_000.0);
        }
        format!("{:.1}G", bits_per_second as f64 / 1_000_000_000.0)
    }

    pub fn elapsed(seconds: f64) -> String {
        let total = seconds.round() as i64;
        let h = total / 3600;
        let m = (total % 3600) / 60;
        let s = total % 60;
        if h > 0 {
            return format!("{h}h {m:02}m {s:02}s");
        }
        if m > 0 {
            return format!("{m}m {s:02}s");
        }
        format!("{s}s")
    }
}

pub mod mac {
    const INVALID: [&str; 3] = [
        "00:00:00:00:00:00",
        "02:00:00:00:00:00",
        "ff:ff:ff:ff:ff:ff",
    ];

    pub fn normalize(value: Option<&str>) -> Option<String> {
        let mut value = value?.trim().to_ascii_lowercase().replace('-', ":");
        if value.contains('.') {
            let compact = value.replace('.', "");
            if compact.len() == 12 {
                value = compact
                    .as_bytes()
                    .chunks(2)
                    .map(|c| std::str::from_utf8(c).unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join(":");
            }
        }
        let octets: Vec<&str> = value.split(':').collect();
        if octets.len() != 6 {
            return None;
        }
        let mut padded = Vec::with_capacity(6);
        for octet in octets {
            if octet.is_empty() || octet.len() > 2 {
                return None;
            }
            if !octet.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            if octet.len() == 1 {
                padded.push(format!("0{octet}"));
            } else {
                padded.push(octet.to_string());
            }
        }
        let joined = padded.join(":");
        if INVALID.contains(&joined.as_str()) {
            return None;
        }
        Some(joined)
    }

    pub fn from_bytes(bytes: &[u8; 6]) -> Option<String> {
        let text = bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":");
        normalize(Some(&text))
    }

    pub fn oui(bssid: Option<&str>) -> Option<String> {
        let bssid = normalize(bssid)?;
        Some(
            bssid
                .split(':')
                .take(3)
                .collect::<Vec<_>>()
                .join(":")
                .to_ascii_uppercase(),
        )
    }
}

pub fn json_line(object: &serde_json::Value) -> String {
    serde_json::to_string(object)
        .unwrap_or_else(|_| "{\"type\":\"log\",\"message\":\"json encoding failed\"}".to_string())
}

/// Returns true when the line already starts with a `YYYY/MM/DD ` stamp.
pub fn starts_with_status_timestamp(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() > 11
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'/'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'/'
        && b[8..10].iter().all(u8::is_ascii_digit)
        && b[10] == b' '
}

pub fn timestamped(line: &str, now: &Date) -> String {
    if starts_with_status_timestamp(line) {
        return line.to_string();
    }
    format!("{}  {}", time::status(now), line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn csv_escaping() {
        assert_eq!(csv::field(Some("plain")), "plain");
        assert_eq!(csv::field(Some("a,b")), "\"a,b\"");
        assert_eq!(csv::field(Some("a\"b")), "\"a\"\"b\"");
        assert_eq!(csv::field(None), "");
    }

    #[test]
    fn units_match_swift() {
        assert_eq!(units::data(0), "0");
        assert_eq!(units::data(512), "512B");
        assert_eq!(units::data(2048), "2K");
        assert_eq!(units::data(1_572_864), "1.5M");
        assert_eq!(units::data(3_221_225_472), "3.0G");
        assert_eq!(units::rate(0), "0");
        assert_eq!(units::rate(950), "950");
        assert_eq!(units::rate(12_000), "12K");
        assert_eq!(units::rate(1_500_000), "1.5M");
        assert_eq!(units::rate(2_000_000_000), "2.0G");
        assert_eq!(units::elapsed(5.0), "5s");
        assert_eq!(units::elapsed(65.0), "1m 05s");
        assert_eq!(units::elapsed(3661.0), "1h 01m 01s");
    }

    #[test]
    fn mac_pads_unpadded_octets() {
        assert_eq!(
            mac::normalize(Some("68:51:34:7c:32:1")).as_deref(),
            Some("68:51:34:7c:32:01")
        );
        assert_eq!(
            mac::normalize(Some("0:b:86:1:2:3")).as_deref(),
            Some("00:0b:86:01:02:03")
        );
        assert_eq!(
            mac::normalize(Some("68-51-34-7C-32-01")).as_deref(),
            Some("68:51:34:7c:32:01")
        );
        assert_eq!(
            mac::normalize(Some("6851.347c.3201")).as_deref(),
            Some("68:51:34:7c:32:01")
        );
    }

    #[test]
    fn mac_rejects_malformed_and_placeholders() {
        assert_eq!(mac::normalize(Some("2:0:0:0:0:0")), None);
        assert_eq!(mac::normalize(Some("68:51:34:7c:32")), None);
        assert_eq!(mac::normalize(Some("68:51:34:7c:32:123")), None);
        assert_eq!(mac::normalize(Some("68:51::7c:32:01")), None);
        assert_eq!(mac::normalize(Some("ff:ff:ff:ff:ff:ff")), None);
        assert_eq!(mac::from_bytes(&[0, 0, 0, 0, 0, 0]), None);
        assert_eq!(
            mac::from_bytes(&[0x00, 0x0b, 0x86, 0x11, 0x22, 0x01]).as_deref(),
            Some("00:0b:86:11:22:01")
        );
        assert_eq!(
            mac::oui(Some("00:0b:86:11:22:01")).as_deref(),
            Some("00:0B:86")
        );
    }

    #[test]
    fn timestamps_use_local_offset() {
        let date = chrono::Local
            .with_ymd_and_hms(2026, 10, 5, 9, 0, 1)
            .unwrap();
        assert_eq!(time::status(&date), "2026/10/05 09:00:01");
        let json = time::json(&date);
        assert!(json.starts_with("2026-10-05T09:00:01.000"));
        assert!(!json.ends_with('Z'));
        let offset = &json[23..];
        assert!(offset.starts_with('+') || offset.starts_with('-'));
        assert_eq!(offset.len(), 6);
    }

    #[test]
    fn style_wraps_codes() {
        assert_eq!(
            style("x", TextColor::Red, true, true),
            "\u{1b}[1;31mx\u{1b}[0m"
        );
        assert_eq!(style("x", TextColor::Red, false, true), "x");
        assert_eq!(style("x", TextColor::None, true, false), "x");
    }

    #[test]
    fn timestamp_prefix_detection() {
        assert!(starts_with_status_timestamp("2026/10/05 09:00:01  JOIN"));
        assert!(!starts_with_status_timestamp("IP  v4 none"));
        let date = chrono::Local
            .with_ymd_and_hms(2026, 10, 5, 9, 0, 1)
            .unwrap();
        assert_eq!(
            timestamped("IP  v4 none", &date),
            "2026/10/05 09:00:01  IP  v4 none"
        );
    }
}
