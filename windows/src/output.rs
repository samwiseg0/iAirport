use serde_json::{json, Map, Value};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::association::{Association, Transition, TransitionKind};
use crate::format::{self, csv, style, time, units, TextColor};
use crate::ip_state::{IpAfterRoam, IpState};
use crate::join_timing::JoinTiming;
use crate::link::{BssidSource, LinkSample, RadioStatus};
use crate::session_log::SessionLog;
use crate::Date;

/// One sampled interval: link state plus counters and IP state.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub timestamp: Date,
    pub status: RadioStatus,
    pub interface_name: String,
    pub link: LinkSample,
    pub vendor: Option<String>,
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
    pub bps_in: Option<u64>,
    pub bps_out: Option<u64>,
    pub ip_state: IpState,
}

impl Sample {
    pub fn new(timestamp: Date, status: RadioStatus, interface_name: &str) -> Self {
        Sample {
            timestamp,
            status,
            interface_name: interface_name.to_string(),
            link: LinkSample::default(),
            vendor: None,
            bytes_in: None,
            bytes_out: None,
            bps_in: None,
            bps_out: None,
            ip_state: IpState::default(),
        }
    }

    fn bssid_source(&self) -> BssidSource {
        self.link.bssid_source.unwrap_or(BssidSource::Bss)
    }
}

pub trait Sink {
    fn write_raw(&mut self, text: &str);
    fn is_tty(&self) -> bool;
    fn columns(&self) -> Option<usize>;
}

pub struct Renderer<S: Sink> {
    pub json_mode: bool,
    pub color_enabled: bool,
    pub session_log: Option<Arc<SessionLog>>,
    sink: S,
    tty: bool,
    last_status: String,
}

impl<S: Sink> Renderer<S> {
    pub fn new(sink: S, no_color: bool, json_mode: bool) -> Self {
        let tty = sink.is_tty() && !json_mode;
        Renderer {
            json_mode,
            color_enabled: tty && !no_color,
            session_log: None,
            sink,
            tty,
            last_status: String::new(),
        }
    }

    pub fn status(&mut self, line: &str, color: TextColor) {
        if self.json_mode {
            return;
        }
        self.log(line);
        // A wrapped status line cannot be redrawn with \r, so clip it to the window.
        let text = if self.tty {
            clip(line, self.sink.columns())
        } else {
            line.to_string()
        };
        let rendered = style(&text, color, self.color_enabled, false);
        if self.tty {
            self.sink.write_raw(&format!("\r\u{001B}[2K{rendered}"));
        } else {
            self.sink.write_raw(&format!("{rendered}\n"));
        }
        self.last_status = rendered;
    }

    pub fn event(&mut self, line: &str, color: TextColor, bold: bool) {
        if self.json_mode {
            return;
        }
        self.log(line);
        let rendered = style(line, color, self.color_enabled, bold);
        if self.tty {
            self.sink.write_raw(&format!("\r\u{001B}[2K{rendered}\n"));
            if !self.last_status.is_empty() {
                self.sink.write_raw(&format!("\r{}", self.last_status));
            }
        } else {
            self.sink.write_raw(&format!("{rendered}\n"));
        }
    }

    pub fn json(&mut self, object: &Value) {
        let line = format::json_line(object);
        self.log(&line);
        self.sink.write_raw(&format!("{line}\n"));
    }

    pub fn finish_line(&mut self) {
        if self.tty {
            self.sink.write_raw("\n");
        }
    }

    pub fn clear_status(&mut self) {
        self.last_status.clear();
        if self.tty {
            self.sink.write_raw("\r\u{001B}[2K");
        }
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    fn log(&self, line: &str) {
        if let Some(log) = &self.session_log {
            log.write(line);
        }
    }
}

pub fn clip(line: &str, columns: Option<usize>) -> String {
    let Some(columns) = columns else {
        return line.to_string();
    };
    let count = line.chars().count();
    if columns <= 1 || count < columns {
        return line.to_string();
    }
    line.chars().take(columns - 1).collect()
}

pub fn status_line(sample: &Sample, verbose: bool) -> (String, TextColor) {
    let ts = time::status(&sample.timestamp);
    match sample.status {
        RadioStatus::Off => (format!("{ts}  Wi-Fi is off"), TextColor::Red),
        RadioStatus::Disconnected => (format!("{ts}  Not connected"), TextColor::Yellow),
        RadioStatus::Associated => {
            let link = &sample.link;
            let ssid = link.ssid.clone().unwrap_or_default();
            let vendor = sample
                .vendor
                .as_ref()
                .map(|v| format!("{v} "))
                .unwrap_or_default();
            let bssid = link.bssid.clone().unwrap_or_default();
            let ap_name = link
                .ap_name
                .as_ref()
                .map(|n| format!(" {n}"))
                .unwrap_or_default();
            let mut parts: Vec<String> = Vec::new();
            let mut channel = String::from("Chan ");
            match link.channel {
                Some(ch) => channel.push_str(&ch.to_string()),
                None => channel.push('?'),
            }
            if let Some(width) = link.width_mhz {
                channel.push_str(&format!("/{width}"));
            }
            parts.push(channel);
            if let Some(phy) = &link.phy {
                parts.push(phy.clone());
            }
            if let Some(sec) = link.security.as_ref().and_then(|s| s.security.clone()) {
                let mut text = sec;
                let info = link.security.as_ref().expect("checked");
                if info.ft {
                    text.push_str(" FT");
                }
                if info.mfp() {
                    text.push_str(" MFP");
                }
                parts.push(text);
            }
            if let Some(rate) = link.tx_rate_mbps.filter(|r| *r > 0.0) {
                parts.push(format!("({rate:.0} Mbps)"));
            }
            let mut quality: Vec<String> = Vec::new();
            // Windows gives no noise floor, SNR or CCA. When the BSS list is
            // unavailable the only strength figure is the driver's percentage.
            match (link.rssi_dbm, link.signal_pct) {
                (Some(rssi), _) => quality.push(format!("RSSI {rssi}")),
                (None, Some(pct)) => quality.push(format!("Signal {pct}%")),
                (None, None) => {}
            }
            let bps = sample.bps_in.unwrap_or(0) + sample.bps_out.unwrap_or(0);
            let bytes = sample.bytes_in.unwrap_or(0) + sample.bytes_out.unwrap_or(0);
            let mut suffix = vec![
                format!("Speed: {}bps", units::rate(bps)),
                format!("Data: {}", units::data(bytes)),
            ];
            if verbose {
                if let Some(rtx) = link.tx_retrans {
                    suffix.push(format!("rtx {rtx}"));
                }
                if let Some(fail) = link.tx_fail {
                    suffix.push(format!("fail {fail}"));
                }
                suffix.push(sample.ip_state.compact_tag());
            }
            let line = format!(
                "{ts}  \"{ssid}\" ({vendor}{bssid}{ap_name}) {}  {}  {}",
                parts.join(" "),
                quality.join(" "),
                suffix.join(" ")
            );
            (line, TextColor::Green)
        }
    }
}

fn ap_display(info: Option<&Association>) -> String {
    let bssid = info.and_then(|i| i.bssid.clone()).unwrap_or_default();
    match info.and_then(|i| i.ap_name.as_ref()) {
        Some(name) => format!("{bssid} ({name})"),
        None => bssid,
    }
}

fn opt<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

pub fn transition_line(transition: &Transition) -> (String, TextColor) {
    let ts = time::status(&transition.at);
    let old = transition.from.as_ref();
    let new = transition.to.as_ref();
    match transition.kind {
        TransitionKind::Join => (
            format!(
                "{ts}  JOIN  {}  \"{}\"  ch {}  RSSI {}",
                ap_display(new),
                new.and_then(|n| n.ssid.clone()).unwrap_or_default(),
                opt(new.and_then(|n| n.channel)),
                opt(new.and_then(|n| n.rssi))
            ),
            TextColor::Cyan,
        ),
        TransitionKind::Roam => (
            format!(
                "{ts}  ROAM  {} -> {}  \"{}\"  ch {} -> {}  RSSI {} -> {}",
                ap_display(old),
                ap_display(new),
                new.and_then(|n| n.ssid.clone())
                    .or_else(|| old.and_then(|o| o.ssid.clone()))
                    .unwrap_or_default(),
                opt(old.and_then(|o| o.channel)),
                opt(new.and_then(|n| n.channel)),
                opt(old.and_then(|o| o.rssi)),
                opt(new.and_then(|n| n.rssi))
            ),
            TextColor::Magenta,
        ),
        TransitionKind::Reconnect => (
            format!(
                "{ts}  RECONNECT  {}  \"{}\"",
                ap_display(new),
                new.and_then(|n| n.ssid.clone()).unwrap_or_default()
            ),
            TextColor::Cyan,
        ),
        TransitionKind::Disconnect => (
            format!(
                "{ts}  DISCONNECT  {}  \"{}\"",
                ap_display(old),
                old.and_then(|o| o.ssid.clone()).unwrap_or_default()
            ),
            TextColor::Yellow,
        ),
    }
}

fn set<T: Into<Value>>(object: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(v) = value {
        object.insert(key.to_string(), v.into());
    }
}

pub fn sample_json(sample: &Sample) -> Value {
    let link = &sample.link;
    let mut object = Map::new();
    object.insert("type".into(), json!("sample"));
    object.insert("ts".into(), json!(time::json(&sample.timestamp)));
    object.insert("state".into(), json!(sample.status.as_str()));
    object.insert("interface".into(), json!(sample.interface_name));
    object.insert("ip".into(), sample.ip_state.json_object());
    set(&mut object, "ssid", link.ssid.as_deref());
    set(&mut object, "bssid", link.bssid.as_deref());
    object.insert("bssid_source".into(), json!(sample.bssid_source().as_str()));
    set(&mut object, "vendor", sample.vendor.as_deref());
    set(&mut object, "ap_name", link.ap_name.as_deref());
    set(&mut object, "channel", link.channel);
    set(&mut object, "width_mhz", link.width_mhz);
    set(&mut object, "band", link.band.as_deref());
    set(&mut object, "phy", link.phy.as_deref());
    set(
        &mut object,
        "security",
        link.security.as_ref().and_then(|s| s.security.as_deref()),
    );
    object.insert(
        "ft".into(),
        json!(link.security.as_ref().is_some_and(|s| s.ft)),
    );
    object.insert(
        "mfp".into(),
        json!(link.security.as_ref().is_some_and(|s| s.mfp())),
    );
    set(&mut object, "tx_rate_mbps", link.tx_rate_mbps);
    set(&mut object, "rssi_dbm", link.rssi_dbm);
    set(&mut object, "signal_pct", link.signal_pct);
    set(&mut object, "bytes_in", sample.bytes_in);
    set(&mut object, "bytes_out", sample.bytes_out);
    set(&mut object, "bps_in", sample.bps_in);
    set(&mut object, "bps_out", sample.bps_out);
    Value::Object(object)
}

pub fn transition_json(transition: &Transition, source: BssidSource) -> Value {
    let old = transition.from.as_ref();
    let new = transition.to.as_ref();
    let mut object = Map::new();
    object.insert("type".into(), json!(transition.kind.as_str()));
    object.insert("ts".into(), json!(time::json(&transition.at)));
    set(
        &mut object,
        "old_bssid",
        old.and_then(|o| o.bssid.as_deref()),
    );
    set(
        &mut object,
        "new_bssid",
        new.and_then(|n| n.bssid.as_deref()),
    );
    set(
        &mut object,
        "old_ap_name",
        old.and_then(|o| o.ap_name.as_deref()),
    );
    set(
        &mut object,
        "new_ap_name",
        new.and_then(|n| n.ap_name.as_deref()),
    );
    object.insert("bssid_source".into(), json!(source.as_str()));
    set(
        &mut object,
        "ssid",
        new.and_then(|n| n.ssid.as_deref())
            .or_else(|| old.and_then(|o| o.ssid.as_deref())),
    );
    set(&mut object, "old_channel", old.and_then(|o| o.channel));
    set(&mut object, "new_channel", new.and_then(|n| n.channel));
    set(&mut object, "old_rssi_dbm", old.and_then(|o| o.rssi));
    set(&mut object, "new_rssi_dbm", new.and_then(|n| n.rssi));
    set(&mut object, "dwell_s", transition.dwell_seconds);
    Value::Object(object)
}

pub fn event_json(kind: &str, message: &str, date: &Date, extra: &[(&str, Value)]) -> Value {
    let mut object = Map::new();
    object.insert("type".into(), json!(kind));
    object.insert("ts".into(), json!(time::json(date)));
    object.insert("message".into(), json!(message));
    for (key, value) in extra {
        object.insert((*key).to_string(), value.clone());
    }
    Value::Object(object)
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoamHistoryEntry {
    pub number: usize,
    pub join_time: Date,
    pub ssid: Option<String>,
    pub bssid: String,
    pub ap_name: Option<String>,
    pub vendor: Option<String>,
    pub channel: Option<u32>,
    pub rssi_at_join: Option<i32>,
    pub rssi_at_leave: Option<i32>,
    pub leave_time: Option<Date>,
}

impl RoamHistoryEntry {
    pub const HEADER: &'static str =
        "#  time  ssid  bssid  ap_name  vendor  ch  rssi_at_join  rssi_at_leave  dwell";

    pub fn line(&self, now: &Date) -> String {
        let leave = self.leave_time.unwrap_or(*now);
        let dwell = units::elapsed(time::seconds_between(&leave, &self.join_time));
        format!(
            "{:2}  {}  {}  {}  {}  {}  {}  {}  {}  {}",
            self.number,
            time::status(&self.join_time),
            self.ssid.clone().unwrap_or_default(),
            self.bssid,
            self.ap_name.clone().unwrap_or_default(),
            self.vendor.clone().unwrap_or_default(),
            opt(self.channel),
            opt(self.rssi_at_join),
            opt(self.rssi_at_leave),
            dwell
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingRoamCsvRow {
    pub transition: Transition,
    pub timing: Option<JoinTiming>,
}

#[derive(Default)]
pub struct PendingRoamCsvBuffer {
    row: Option<PendingRoamCsvRow>,
}

impl PendingRoamCsvBuffer {
    pub fn replace(
        &mut self,
        transition: Transition,
        timing: Option<JoinTiming>,
    ) -> Option<PendingRoamCsvRow> {
        self.row.replace(PendingRoamCsvRow { transition, timing })
    }

    pub fn take(&mut self) -> Option<PendingRoamCsvRow> {
        self.row.take()
    }
}

pub const SAMPLE_HEADER: &str = "ts_iso,epoch_ms,state,ssid,bssid,bssid_source,ap_name,vendor,channel,width_mhz,band,phy,security,ft,mcs,nss,gi_ns,tx_rate_mbps,rssi_dbm,noise_dbm,snr_db,cca_pct,tx_retrans,tx_fail,rx_retry,bytes_in,bytes_out,bps_in,bps_out,ipv4,ipv4_gw,ipv6,ipv6_kind,ipv6_gw,ipv6_count";
pub const ROAM_HEADER: &str = "ts_iso,epoch_ms,kind,ssid,old_bssid,new_bssid,old_ap_name,new_ap_name,bssid_source,old_channel,new_channel,old_rssi_dbm,new_rssi_dbm,dwell_s,v4_after_roam,v6_after_roam,v4_ready_ms,v6_ready_ms,assoc_ms,auth_ms,linkup_ms,ipv4_ms,ipv6_ms,ipv4_primary_ms,ipv6_primary_ms";

fn s<T: ToString>(value: Option<T>) -> Option<String> {
    value.map(|v| v.to_string())
}

/// The columns mcs, nss, gi_ns, noise_dbm, snr_db and cca_pct stay empty.
/// wlanapi exposes none of them.
pub fn sample_row(sample: &Sample) -> String {
    let link = &sample.link;
    let v6 = sample.ip_state.display_ipv6();
    let ip = &sample.ip_state;
    csv::row(&[
        Some(time::csv(&sample.timestamp)),
        Some(time::epoch_ms(&sample.timestamp)),
        Some(sample.status.as_str().to_string()),
        link.ssid.clone(),
        link.bssid.clone(),
        Some(sample.bssid_source().as_str().to_string()),
        link.ap_name.clone(),
        sample.vendor.clone(),
        s(link.channel),
        s(link.width_mhz),
        link.band.clone(),
        link.phy.clone(),
        link.security.as_ref().and_then(|x| x.security.clone()),
        Some(
            if link.security.as_ref().is_some_and(|x| x.ft) {
                "1"
            } else {
                "0"
            }
            .to_string(),
        ),
        None,
        None,
        None,
        link.tx_rate_mbps.map(|r| format!("{r:.1}")),
        s(link.rssi_dbm),
        None,
        None,
        None,
        s(link.tx_retrans),
        s(link.tx_fail),
        s(link.rx_retry),
        s(sample.bytes_in),
        s(sample.bytes_out),
        s(sample.bps_in),
        s(sample.bps_out),
        Some(ip.ipv4.join(" ")),
        ip.ipv4_router.clone(),
        v6.map(|a| a.address.clone()),
        v6.map(|a| a.kind().as_str().to_string()),
        ip.ipv6_router.clone(),
        Some(ip.ipv6.len().to_string()),
    ])
}

pub fn transition_row(
    transition: &Transition,
    source: BssidSource,
    timing: Option<&JoinTiming>,
    v4: Option<IpAfterRoam>,
    v6: Option<IpAfterRoam>,
) -> String {
    let old = transition.from.as_ref();
    let new = transition.to.as_ref();
    csv::row(&[
        Some(time::csv(&transition.at)),
        Some(time::epoch_ms(&transition.at)),
        Some(transition.kind.as_str().to_string()),
        new.and_then(|n| n.ssid.clone())
            .or_else(|| old.and_then(|o| o.ssid.clone())),
        old.and_then(|o| o.bssid.clone()),
        new.and_then(|n| n.bssid.clone()),
        old.and_then(|o| o.ap_name.clone()),
        new.and_then(|n| n.ap_name.clone()),
        Some(source.as_str().to_string()),
        s(old.and_then(|o| o.channel)),
        s(new.and_then(|n| n.channel)),
        s(old.and_then(|o| o.rssi)),
        s(new.and_then(|n| n.rssi)),
        transition.dwell_seconds.map(|d| format!("{d:.1}")),
        v4.map(|v| v.csv_value().to_string()),
        v6.map(|v| v.csv_value().to_string()),
        s(v4.and_then(|v| v.milliseconds())),
        s(v6.and_then(|v| v.milliseconds())),
        s(timing.and_then(|t| t.assoc)),
        s(timing.and_then(|t| t.auth)),
        s(timing.and_then(|t| t.linkup)),
        s(timing.and_then(|t| t.ipv4)),
        s(timing.and_then(|t| t.ipv6)),
        s(timing.and_then(|t| t.ipv4_primary)),
        s(timing.and_then(|t| t.ipv6_primary)),
    ])
}

pub struct CsvLogger {
    samples: Option<File>,
    roams: Option<File>,
    bssids: Option<File>,
}

impl CsvLogger {
    pub fn open(directory: &Path, now: &Date, warnings: &mut Vec<String>) -> CsvLogger {
        CsvLogger {
            samples: open_csv(
                &directory.join("iairport-samples.csv"),
                SAMPLE_HEADER,
                now,
                warnings,
            ),
            roams: open_csv(
                &directory.join("iairport-roams.csv"),
                ROAM_HEADER,
                now,
                warnings,
            ),
            bssids: OpenOptions::new()
                .append(true)
                .create(true)
                .open(directory.join("bssid_list.txt"))
                .ok(),
        }
    }

    pub fn write_sample(&mut self, sample: &Sample) {
        if let Some(file) = self.samples.as_mut() {
            let _ = file.write_all(sample_row(sample).as_bytes());
        }
    }

    pub fn write_transition(
        &mut self,
        transition: &Transition,
        source: BssidSource,
        timing: Option<&JoinTiming>,
        v4: Option<IpAfterRoam>,
        v6: Option<IpAfterRoam>,
    ) {
        if let Some(file) = self.roams.as_mut() {
            let _ = file.write_all(transition_row(transition, source, timing, v4, v6).as_bytes());
        }
    }

    pub fn write_bssid(&mut self, bssid: Option<&str>) {
        if let (Some(file), Some(bssid)) = (self.bssids.as_mut(), bssid) {
            let _ = file.write_all(format!("{bssid}\n").as_bytes());
        }
    }

    pub fn close(&mut self) {
        for file in [self.samples.take(), self.roams.take(), self.bssids.take()]
            .into_iter()
            .flatten()
        {
            let _ = (&file).flush();
        }
    }
}

/// Appends to an existing file whose first line equals `header`. A file
/// with a different header is left alone and a `<name>.<epoch>.csv`
/// sibling is written instead, as on macOS.
fn open_csv(path: &Path, header: &str, now: &Date, warnings: &mut Vec<String>) -> Option<File> {
    let mut final_path: PathBuf = path.to_path_buf();
    if path.exists() {
        if let Some(first) = first_line(path) {
            if !first.is_empty() && first != header {
                let name = format!("{}.{}.csv", path.display(), now.timestamp());
                final_path = PathBuf::from(&name);
                warnings.push(format!(
                    "warning: {} header mismatch, writing {}",
                    path.display(),
                    final_path.display()
                ));
            }
        }
    }
    let needs_header = !final_path.exists()
        || std::fs::metadata(&final_path)
            .map(|m| m.len() == 0)
            .unwrap_or(true);
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&final_path)
        .ok()?;
    if needs_header {
        file.write_all(format!("{header}\n").as_bytes()).ok()?;
    }
    Some(file)
}

fn first_line(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buffer = vec![0u8; 4096];
    let read = file.read(&mut buffer).ok()?;
    let text = String::from_utf8_lossy(&buffer[..read]);
    text.split('\n')
        .next()
        .map(|l| l.trim_end_matches('\r').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ie::SecurityInfo;
    use crate::ip_state::{Ipv6AddressInfo, Ipv6Origin};
    use chrono::{Local, TimeZone};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn at(h: u32, m: u32, s: u32) -> Date {
        Local.with_ymd_and_hms(2026, 10, 5, h, m, s).unwrap()
    }

    fn sample() -> Sample {
        let mut sample = Sample::new(at(9, 0, 1), RadioStatus::Associated, "Wi-Fi");
        sample.link = LinkSample {
            ssid: Some("Corp".into()),
            bssid: Some("00:0b:86:11:22:01".into()),
            bssid_source: Some(BssidSource::Bss),
            ap_name: Some("ap-3-2".into()),
            channel: Some(36),
            width_mhz: Some(80),
            band: Some("5".into()),
            phy: Some("802.11ax".into()),
            security: Some(SecurityInfo {
                security: Some("wpa2-enterprise".into()),
                ft: true,
                mfp_capable: true,
                mfp_required: false,
            }),
            tx_rate_mbps: Some(1200.0),
            rssi_dbm: Some(-55),
            signal_pct: Some(90),
            tx_retrans: Some(3),
            tx_fail: Some(0),
            rx_retry: None,
        };
        sample.vendor = Some("Aruba".into());
        sample.bytes_in = Some(1_200_000);
        sample.bytes_out = Some(300_000);
        sample.bps_in = Some(10_000_000);
        sample.bps_out = Some(2_000_000);
        sample.ip_state = IpState {
            ipv4: vec!["192.0.2.143".into()],
            ipv4_prefixes: vec![Some(24)],
            ipv4_router: Some("192.0.2.1".into()),
            ipv6: vec![Ipv6AddressInfo::new(
                "2001:db8:1::1234",
                Some(64),
                Ipv6Origin {
                    prefix_origin: 4,
                    suffix_origin: 4,
                    dad_state: 4,
                    preferred_lifetime: 100,
                },
            )],
            ipv6_router: Some("fe80::1".into()),
            primary_v4: true,
            primary_v6: true,
        };
        sample
    }

    #[test]
    fn status_line_formats() {
        let (line, color) = status_line(&sample(), false);
        assert_eq!(
            line,
            "2026/10/05 09:00:01  \"Corp\" (Aruba 00:0b:86:11:22:01 ap-3-2) Chan 36/80 802.11ax wpa2-enterprise FT MFP (1200 Mbps)  RSSI -55  Speed: 12.0Mbps Data: 1.4M"
        );
        assert_eq!(color, TextColor::Green);
        let (verbose, _) = status_line(&sample(), true);
        assert!(verbose.ends_with("Speed: 12.0Mbps Data: 1.4M rtx 3 fail 0 v4 v6"));
        let mut reduced = sample();
        reduced.link.rssi_dbm = None;
        reduced.link.ap_name = None;
        reduced.link.bssid_source = Some(BssidSource::Connection);
        let (line, _) = status_line(&reduced, false);
        assert!(line.contains("(Aruba 00:0b:86:11:22:01) Chan 36/80"));
        assert!(line.contains("  Signal 90%  Speed:"));
        let off = Sample::new(at(9, 0, 1), RadioStatus::Off, "Wi-Fi");
        assert_eq!(
            status_line(&off, false),
            (
                "2026/10/05 09:00:01  Wi-Fi is off".to_string(),
                TextColor::Red
            )
        );
        let disc = Sample::new(at(9, 0, 1), RadioStatus::Disconnected, "Wi-Fi");
        assert_eq!(
            status_line(&disc, false),
            (
                "2026/10/05 09:00:01  Not connected".to_string(),
                TextColor::Yellow
            )
        );
        let mut unknown = sample();
        unknown.link.channel = None;
        unknown.link.width_mhz = None;
        assert!(status_line(&unknown, false).0.contains(") Chan ? 802.11ax"));
    }

    fn assoc(bssid: &str, name: Option<&str>, ch: u32, rssi: i32) -> Association {
        Association {
            ssid: Some("Corp".into()),
            bssid: Some(bssid.into()),
            ap_name: name.map(|n| n.into()),
            channel: Some(ch),
            rssi: Some(rssi),
            since: at(8, 59, 0),
        }
    }

    #[test]
    fn transition_lines_match_docs() {
        let join = Transition {
            kind: TransitionKind::Join,
            from: None,
            to: Some(assoc("00:0b:86:11:22:01", Some("ap-3-2"), 36, -55)),
            dwell_seconds: None,
            at: at(9, 0, 1),
        };
        assert_eq!(
            transition_line(&join).0,
            "2026/10/05 09:00:01  JOIN  00:0b:86:11:22:01 (ap-3-2)  \"Corp\"  ch 36  RSSI -55"
        );
        let roam = Transition {
            kind: TransitionKind::Roam,
            from: Some(assoc("00:0b:86:11:22:01", Some("ap-3-2"), 36, -71)),
            to: Some(assoc("00:0b:86:11:22:02", Some("ap-3-3"), 44, -58)),
            dwell_seconds: Some(14.2),
            at: at(9, 0, 15),
        };
        let (line, color) = transition_line(&roam);
        assert_eq!(line, "2026/10/05 09:00:15  ROAM  00:0b:86:11:22:01 (ap-3-2) -> 00:0b:86:11:22:02 (ap-3-3)  \"Corp\"  ch 36 -> 44  RSSI -71 -> -58");
        assert_eq!(color, TextColor::Magenta);
        let re = Transition {
            kind: TransitionKind::Reconnect,
            from: None,
            to: Some(assoc("00:0b:86:11:22:02", None, 44, -60)),
            dwell_seconds: None,
            at: at(9, 1, 0),
        };
        assert_eq!(
            transition_line(&re).0,
            "2026/10/05 09:01:00  RECONNECT  00:0b:86:11:22:02  \"Corp\""
        );
        let disc = Transition {
            kind: TransitionKind::Disconnect,
            from: Some(assoc("00:0b:86:11:22:02", Some("ap-3-3"), 44, -60)),
            to: None,
            dwell_seconds: Some(3.0),
            at: at(9, 2, 0),
        };
        assert_eq!(
            transition_line(&disc).0,
            "2026/10/05 09:02:00  DISCONNECT  00:0b:86:11:22:02 (ap-3-3)  \"Corp\""
        );
        let json = transition_json(&roam, BssidSource::Bss);
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "bssid_source",
                "dwell_s",
                "new_ap_name",
                "new_bssid",
                "new_channel",
                "new_rssi_dbm",
                "old_ap_name",
                "old_bssid",
                "old_channel",
                "old_rssi_dbm",
                "ssid",
                "ts",
                "type"
            ]
        );
        assert_eq!(json["type"], "roam");
        let row = transition_row(
            &roam,
            BssidSource::Bss,
            Some(&JoinTiming {
                assoc: Some(272),
                ..Default::default()
            }),
            Some(IpAfterRoam::Kept),
            Some(IpAfterRoam::Changed(2100)),
        );
        assert!(row.ends_with(",Corp,00:0b:86:11:22:01,00:0b:86:11:22:02,ap-3-2,ap-3-3,bss,36,44,-71,-58,14.2,kept,changed,,2100,272,,,,,,\n"), "{row}");
        assert_eq!(row.split(',').count(), ROAM_HEADER.split(',').count());
    }

    #[test]
    fn sample_json_keys_and_csv_columns() {
        let json = sample_json(&sample());
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "ap_name",
                "band",
                "bps_in",
                "bps_out",
                "bssid",
                "bssid_source",
                "bytes_in",
                "bytes_out",
                "channel",
                "ft",
                "interface",
                "ip",
                "mfp",
                "phy",
                "rssi_dbm",
                "security",
                "signal_pct",
                "ssid",
                "state",
                "ts",
                "tx_rate_mbps",
                "type",
                "vendor",
                "width_mhz"
            ]
        );
        assert_eq!(json["ft"], true);
        assert_eq!(json["mfp"], true);
        assert_eq!(json["state"], "associated");
        assert_eq!(json["ip"]["v4"][0], "192.0.2.143");
        let row = sample_row(&sample());
        assert_eq!(row.split(',').count(), SAMPLE_HEADER.split(',').count());
        assert!(row.contains(",associated,Corp,00:0b:86:11:22:01,bss,ap-3-2,Aruba,36,80,5,802.11ax,wpa2-enterprise,1,,,,1200.0,-55,,,,3,0,,1200000,300000,10000000,2000000,192.0.2.143,192.0.2.1,2001:db8:1::1234,slaac,fe80::1,1\n"), "{row}");
        let event = event_json(
            "deauth",
            "DEAUTH  reason 3",
            &at(9, 0, 10),
            &[("reason", json!(3))],
        );
        assert_eq!(
            format::json_line(&event),
            "{\"message\":\"DEAUTH  reason 3\",\"reason\":3,\"ts\":\"2026-10-05T09:00:10.000"
                .to_string()
                + &time::json(&at(9, 0, 10))[23..]
                + "\",\"type\":\"deauth\"}"
        );
    }

    #[test]
    fn headers_are_documented_strings() {
        assert_eq!(SAMPLE_HEADER.split(',').count(), 35);
        assert_eq!(ROAM_HEADER.split(',').count(), 25);
        assert!(SAMPLE_HEADER.starts_with("ts_iso,epoch_ms,state,ssid,bssid,bssid_source,"));
        assert!(ROAM_HEADER.ends_with(",ipv4_primary_ms,ipv6_primary_ms"));
    }

    #[test]
    fn history_line_and_pending_buffer() {
        let entry = RoamHistoryEntry {
            number: 1,
            join_time: at(9, 0, 1),
            ssid: Some("Corp".into()),
            bssid: "00:0b:86:11:22:01".into(),
            ap_name: Some("ap-3-2".into()),
            vendor: Some("Aruba".into()),
            channel: Some(36),
            rssi_at_join: Some(-55),
            rssi_at_leave: Some(-71),
            leave_time: Some(at(9, 0, 15)),
        };
        assert_eq!(
            entry.line(&at(9, 5, 0)),
            " 1  2026/10/05 09:00:01  Corp  00:0b:86:11:22:01  ap-3-2  Aruba  36  -55  -71  14s"
        );
        assert_eq!(
            RoamHistoryEntry::HEADER,
            "#  time  ssid  bssid  ap_name  vendor  ch  rssi_at_join  rssi_at_leave  dwell"
        );
        let mut buffer = PendingRoamCsvBuffer::default();
        let t = Transition {
            kind: TransitionKind::Join,
            from: None,
            to: None,
            dwell_seconds: None,
            at: at(9, 0, 1),
        };
        assert!(buffer.replace(t.clone(), None).is_none());
        assert!(buffer.replace(t.clone(), None).is_some());
        assert!(buffer.take().is_some());
        assert!(buffer.take().is_none());
    }

    #[derive(Clone, Default)]
    struct Buffer {
        text: Rc<RefCell<String>>,
        tty: bool,
        columns: Option<usize>,
    }

    impl Sink for Buffer {
        fn write_raw(&mut self, text: &str) {
            self.text.borrow_mut().push_str(text);
        }
        fn is_tty(&self) -> bool {
            self.tty
        }
        fn columns(&self) -> Option<usize> {
            self.columns
        }
    }

    #[test]
    fn renderer_clips_and_redraws() {
        assert_eq!(clip("abcdef", Some(4)), "abc");
        assert_eq!(clip("abc", Some(4)), "abc");
        assert_eq!(clip("abcdef", None), "abcdef");
        let buffer = Buffer {
            tty: true,
            columns: Some(10),
            ..Default::default()
        };
        let mut r = Renderer::new(buffer.clone(), true, false);
        r.status("0123456789abc", TextColor::Green);
        r.event("EVENT", TextColor::Cyan, false);
        assert_eq!(
            *buffer.text.borrow(),
            "\r\u{1b}[2K012345678\r\u{1b}[2KEVENT\n\r012345678"
        );
        let plain = Buffer::default();
        let mut p = Renderer::new(plain.clone(), false, false);
        p.status("status", TextColor::Green);
        p.event("event", TextColor::Cyan, true);
        assert_eq!(*plain.text.borrow(), "status\nevent\n");
        let json = Buffer {
            tty: true,
            ..Default::default()
        };
        let mut j = Renderer::new(json.clone(), false, true);
        j.status("hidden", TextColor::Green);
        j.json(&json!({"type": "x"}));
        assert_eq!(*json.text.borrow(), "{\"type\":\"x\"}\n");
        assert!(!j.color_enabled);
    }

    #[test]
    fn csv_logger_header_mismatch() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("csv-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("iairport-samples.csv"), "old,header\n").unwrap();
        let mut warnings = Vec::new();
        let now = Local::now();
        let mut logger = CsvLogger::open(&dir, &now, &mut warnings);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("header mismatch"));
        logger.write_sample(&sample());
        logger.write_bssid(Some("00:0b:86:11:22:01"));
        logger.close();
        let renamed = dir.join(format!("iairport-samples.csv.{}.csv", now.timestamp()));
        let text = std::fs::read_to_string(&renamed).unwrap();
        assert!(text.starts_with(SAMPLE_HEADER));
        assert_eq!(text.lines().count(), 2);
        assert_eq!(
            std::fs::read_to_string(dir.join("iairport-roams.csv")).unwrap(),
            format!("{ROAM_HEADER}\n")
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("bssid_list.txt")).unwrap(),
            "00:0b:86:11:22:01\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
