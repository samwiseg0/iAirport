//! Port of the macOS Monitor state machine. Every method takes `now` and
//! the host layer feeds samples and events in, so the whole flow runs in
//! tests with a buffer sink.

use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

use crate::association::{AssociationTracker, Transition, TransitionKind};
use crate::cli::RunOptions;
use crate::events::{LogEvent, Severity, WlanEvent};
use crate::format::{self, time, units, TextColor};
use crate::ip_state::{self, IpState};
use crate::join_timing::{JoinTiming, JoinTimingTracker};
use crate::link::{BssidSource, LinkState, RadioStatus};
use crate::oui::Oui;
use crate::output::{
    self, CsvLogger, PendingRoamCsvBuffer, Renderer, RoamHistoryEntry, Sample, Sink,
};
use crate::reason_codes;
use crate::session_log::SessionLog;
use crate::throughput::{CounterSample, CounterTracker};
use crate::{Date, VERSION};

pub const LOCATION_HINT: &str = "Wi-Fi details withheld (access denied). Windows 11 24H2 and later need Location on: Settings > Privacy & security > Location, turn on Location services and \"Let desktop apps access your location\". Running in reduced mode: SSID, BSSID, AP name and RSSI in dBm can be blank, bssid_source=connection.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleReason {
    Startup,
    Poll,
    Settle,
}

/// What the host layer read for one sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub link: LinkState,
    pub ip: IpState,
    pub counters: Option<CounterSample>,
    /// True when `WlanGetNetworkBssList` returned ERROR_ACCESS_DENIED.
    pub bss_denied: bool,
}

struct PendingRoamIp {
    before: IpState,
    latest: IpState,
    start: Date,
    last_change: Date,
}

struct PendingTransitionIp {
    deadline: Date,
    latest: IpState,
}

/// A JOIN or RECONNECT row held back until its JOIN TIMING is known. The
/// poll sees the join before the timing completes, so the row cannot be
/// written at transition time.
struct PendingJoinCsv {
    transition: Transition,
    source: BssidSource,
    deadline: Date,
}

/// Longer than `join_timing::REPORT_AFTER_MS`, so a partial timing still
/// reaches the row.
const JOIN_ROW_HOLD_SECS: i64 = 20;

pub struct Monitor<S: Sink> {
    options: RunOptions,
    interface_name: String,
    renderer: Renderer<S>,
    session_log: Option<Arc<SessionLog>>,
    session_log_warning: Option<String>,
    oui: Oui,
    oui_warning: Option<String>,
    os_line: String,
    csv: Option<CsvLogger>,
    association: AssociationTracker,
    counters: CounterTracker,
    join_timing: JoinTimingTracker,
    last_sample: Option<Sample>,
    last_counter_at: Option<Date>,
    last_observed_ip: Option<IpState>,
    last_ip_line: Option<String>,
    pending_ip_event: Option<IpState>,
    ip_last_changed_at: Option<Date>,
    pending_roam_ip: Option<PendingRoamIp>,
    pending_roam_csv: PendingRoamCsvBuffer,
    pending_join_csv: Option<PendingJoinCsv>,
    pending_transition_ip: Option<PendingTransitionIp>,
    dhcp_deadline: Option<Date>,
    dhcpv6_deadline: Option<Date>,
    settle_times: Vec<Date>,
    start_date: Date,
    roams: usize,
    reconnects: usize,
    disconnects: usize,
    distinct_bssids: HashSet<String>,
    history: Vec<RoamHistoryEntry>,
    current_history_index: Option<usize>,
    bssid_source: BssidSource,
    location_hint_printed: bool,
    shutting_down: bool,
}

pub struct MonitorConfig {
    pub options: RunOptions,
    pub interface_name: String,
    pub os_line: String,
    pub oui: Oui,
    pub oui_warning: Option<String>,
    pub session_log: Option<Arc<SessionLog>>,
    pub session_log_warning: Option<String>,
}

impl<S: Sink> Monitor<S> {
    pub fn new(config: MonitorConfig, sink: S, now: Date) -> Self {
        let mut renderer = Renderer::new(sink, config.options.no_color, config.options.json);
        renderer.session_log = config.session_log.clone();
        Monitor {
            options: config.options,
            interface_name: config.interface_name,
            renderer,
            session_log: config.session_log,
            session_log_warning: config.session_log_warning,
            oui: config.oui,
            oui_warning: config.oui_warning,
            os_line: config.os_line,
            csv: None,
            association: AssociationTracker::default(),
            counters: CounterTracker::default(),
            join_timing: JoinTimingTracker::default(),
            last_sample: None,
            last_counter_at: None,
            last_observed_ip: None,
            last_ip_line: None,
            pending_ip_event: None,
            ip_last_changed_at: None,
            pending_roam_ip: None,
            pending_roam_csv: PendingRoamCsvBuffer::default(),
            pending_join_csv: None,
            pending_transition_ip: None,
            dhcp_deadline: None,
            dhcpv6_deadline: None,
            settle_times: Vec::new(),
            start_date: now,
            roams: 0,
            reconnects: 0,
            disconnects: 0,
            distinct_bssids: HashSet::new(),
            history: Vec::new(),
            current_history_index: None,
            bssid_source: BssidSource::Bss,
            location_hint_printed: false,
            shutting_down: false,
        }
    }

    pub fn renderer(&self) -> &Renderer<S> {
        &self.renderer
    }

    pub fn start(&mut self, now: Date) {
        self.start_date = now;
        self.print_header(now);
        if self.options.log {
            let mut warnings = Vec::new();
            let dir = std::env::current_dir().unwrap_or_default();
            self.csv = Some(CsvLogger::open(&dir, &now, &mut warnings));
            for warning in warnings {
                self.write_event(&warning, TextColor::Yellow, "log", None, false, now);
            }
        }
    }

    #[cfg(test)]
    fn set_csv(&mut self, csv: CsvLogger) {
        self.csv = Some(csv);
    }

    fn print_header(&mut self, now: Date) {
        if self.options.json {
            if let Some(warning) = self.session_log_warning.clone() {
                let message = format!("warning: session log unavailable ({warning})");
                self.renderer
                    .json(&output::event_json("log", &message, &now, &[]));
            }
            return;
        }
        let header = format!("iairport v{VERSION} (Windows port of iAirport by Guillaume Germain)");
        self.renderer.event(&header, TextColor::None, false);
        let os = self.os_line.clone();
        self.renderer.event(&os, TextColor::None, false);
        let iface = format!("interface {}", self.interface_name);
        self.renderer.event(&iface, TextColor::None, false);
        if let Some(log) = &self.session_log {
            let line = format!("Session log: {}", log.path.display());
            self.renderer.event(&line, TextColor::None, false);
        }
        if let Some(warning) = self.session_log_warning.clone() {
            let line = format!("warning: session log unavailable ({warning})");
            self.renderer.event(&line, TextColor::Yellow, false);
        }
        if let Some(warning) = self.oui_warning.clone() {
            let line = format!("warning: {warning}");
            self.renderer.event(&line, TextColor::Yellow, false);
        }
        if self.options.log {
            self.renderer.event(
                "Logging to iairport-samples.csv, iairport-roams.csv and bssid_list.txt",
                TextColor::None,
                false,
            );
        }
    }

    pub fn warn(&mut self, line: &str, now: Date) {
        self.write_event(line, TextColor::Yellow, "log", None, false, now);
    }

    pub fn verbose(&mut self, line: &str, now: Date) {
        if self.options.verbose {
            self.write_event(line, TextColor::Dim, "log", None, false, now);
        }
    }

    /// Earliest moment a timer needs a sample. The host sleeps until this
    /// or the next poll, whichever comes first.
    pub fn next_deadline(&self) -> Option<Date> {
        let mut candidates: Vec<Date> = self.settle_times.clone();
        if let Some(changed) = self.ip_last_changed_at {
            candidates.push(changed + chrono::Duration::seconds(1));
        }
        if let Some(pending) = &self.pending_roam_ip {
            candidates.push(pending.last_change + chrono::Duration::seconds(1));
        }
        if let Some(pending) = &self.pending_transition_ip {
            candidates.push(pending.deadline);
        }
        if let Some(d) = self.dhcp_deadline {
            candidates.push(d);
        }
        if let Some(d) = self.dhcpv6_deadline {
            candidates.push(d);
        }
        if let Some(pending) = &self.pending_join_csv {
            candidates.push(pending.deadline);
        }
        if self.join_timing.in_progress() {
            candidates.push(self.start_date.max(self.join_timing_deadline()));
        }
        candidates.into_iter().min()
    }

    fn join_timing_deadline(&self) -> Date {
        self.last_sample
            .as_ref()
            .map(|s| s.timestamp + chrono::Duration::seconds(1))
            .unwrap_or(self.start_date)
    }

    pub fn schedule_settle_reads(&mut self, now: Date) {
        for ms in [0, 500, 2000, 3000] {
            self.settle_times
                .push(now + chrono::Duration::milliseconds(ms));
        }
    }

    pub fn on_wlan_event(&mut self, event: WlanEvent, now: Date) {
        if self.shutting_down {
            return;
        }
        match &event {
            WlanEvent::Associating => {
                self.join_timing.associating(now);
                self.verbose("wlan msm associating", now);
            }
            WlanEvent::Associated { bssid } => {
                self.join_timing.associated(now);
                let line = format!("wlan msm associated  {}", bssid.clone().unwrap_or_default());
                self.verbose(&line, now);
            }
            WlanEvent::Authenticating => self.verbose("wlan msm authenticating", now),
            WlanEvent::Connected { bssid } => {
                self.join_timing.connected(now);
                let line = format!("wlan msm connected  {}", bssid.clone().unwrap_or_default());
                self.verbose(&line, now);
            }
            WlanEvent::Disassociating => self.verbose("wlan msm disassociating", now),
            WlanEvent::ConnectionComplete => {
                self.join_timing.connection_complete(now);
                self.verbose("wlan connection complete", now);
            }
            WlanEvent::Disconnecting => self.verbose("wlan disconnecting", now),
            WlanEvent::Disconnected {
                reason,
                reason_text,
            } => {
                self.close_join_timing(now);
                // WLAN_REASON_CODE namespace. Printed under its own label so
                // nobody reads it as an 802.11 deauth reason.
                let line = format!("WLAN DISCONNECTED  reason {reason} {reason_text}");
                let json = output::event_json(
                    "wlan_reason",
                    &line,
                    &now,
                    &[
                        ("reason", json!(reason)),
                        ("reason_text", json!(reason_text)),
                    ],
                );
                self.write_event(
                    &line,
                    TextColor::Yellow,
                    "wlan_reason",
                    Some(json),
                    false,
                    now,
                );
            }
            WlanEvent::ConnectionAttemptFail {
                reason,
                reason_text,
            } => {
                self.join_timing.reset();
                let line = format!("WLAN CONNECT FAILED  reason {reason} {reason_text}");
                let json = output::event_json(
                    "wlan_reason",
                    &line,
                    &now,
                    &[
                        ("reason", json!(reason)),
                        ("reason_text", json!(reason_text)),
                    ],
                );
                self.write_event(&line, TextColor::Red, "wlan_reason", Some(json), false, now);
            }
            WlanEvent::MsmDisconnected {
                bssid,
                reason,
                reason_text,
            } => {
                let line = format!(
                    "wlan msm disconnected  {}  reason {reason} {reason_text}",
                    bssid.clone().unwrap_or_default()
                );
                self.verbose(&line, now);
            }
            WlanEvent::RoamingStart => {
                self.begin_join_timing(now);
                self.verbose("wlan roaming start", now)
            }
            WlanEvent::RoamingEnd => self.verbose("wlan roaming end", now),
            WlanEvent::ConnectionStart => {
                self.begin_join_timing(now);
                self.verbose("wlan connection start", now)
            }
            WlanEvent::Other(text) => {
                let line = format!("wlan {text}");
                self.verbose(&line, now);
            }
            _ => {}
        }
        if event.settle_reason().is_some() {
            self.schedule_settle_reads(now);
        }
    }

    pub fn on_log_event(&mut self, event: LogEvent, now: Date) {
        if self.shutting_down {
            return;
        }
        match event {
            LogEvent::Deauth {
                kind,
                source,
                reason,
            } => {
                let text = reason_codes::text(reason);
                let line = format!(
                    "{}  {}",
                    time::status(&now),
                    reason_codes::deauth_line(&kind, reason, source.as_deref())
                );
                let json = output::event_json(
                    "deauth",
                    &line,
                    &now,
                    &[("reason", json!(reason)), ("reason_text", json!(text))],
                );
                self.write_event(&line, TextColor::Red, "deauth", Some(json), false, now);
            }
            LogEvent::Dhcp => self.dhcp_deadline = Some(now + chrono::Duration::seconds(2)),
            LogEvent::Dhcpv6 => self.dhcpv6_deadline = Some(now + chrono::Duration::seconds(2)),
            LogEvent::Line { text, severity } => {
                let color = match severity {
                    Severity::Info => TextColor::Cyan,
                    Severity::Warning => TextColor::Yellow,
                    Severity::Error => TextColor::Red,
                };
                self.write_event(&text, color, "log", None, false, now);
            }
            LogEvent::Verbose(text) => self.verbose(&text, now),
        }
    }

    pub fn sample(&mut self, reading: Reading, reason: SampleReason, now: Date) {
        if self.shutting_down {
            return;
        }
        self.settle_times.retain(|t| *t > now);
        let emit_status = matches!(reason, SampleReason::Startup | SampleReason::Poll);
        if reading.bss_denied && !self.location_hint_printed {
            self.location_hint_printed = true;
            self.write_event(LOCATION_HINT, TextColor::Yellow, "log", None, false, now);
        }
        let mut sample = Sample::new(now, reading.link.status, &self.interface_name);
        sample.link = reading.link.link.clone();
        if let Some(source) = sample.link.bssid_source {
            self.bssid_source = source;
        } else if sample.status == RadioStatus::Associated {
            sample.link.bssid_source = Some(self.bssid_source);
        }
        sample.vendor = self.oui.vendor(sample.link.bssid.as_deref());
        if let Some(counters) = reading.counters {
            let seconds = self
                .last_counter_at
                .map(|t| time::seconds_between(&now, &t))
                .unwrap_or(0.0);
            let delta = self.counters.update(counters, seconds);
            self.last_counter_at = Some(now);
            sample.bytes_in = Some(self.counters.total_in);
            sample.bytes_out = Some(self.counters.total_out);
            sample.bps_in = Some(delta.bps_in);
            sample.bps_out = Some(delta.bps_out);
        }
        sample.ip_state = reading.ip.clone();

        let before_ip = self.last_observed_ip.clone();
        let transition = self.association.commit(&reading.link, now);
        if sample.status == RadioStatus::Associated {
            if let Some(bssid) = &sample.link.bssid {
                self.distinct_bssids.insert(bssid.clone());
            }
            if transition.is_none() {
                self.association.update_rssi(sample.link.rssi_dbm);
            }
        }
        self.handle_ip_observation(&reading.ip, now, transition.is_some());
        if let Some(transition) = &transition {
            self.handle_transition(transition, &sample, before_ip.as_ref());
        }
        self.advance_join_timing(&sample, now);
        self.last_sample = Some(sample.clone());
        if emit_status {
            if let Some(csv) = self.csv.as_mut() {
                csv.write_sample(&sample);
            }
            if self.options.json {
                self.renderer.json(&output::sample_json(&sample));
            } else {
                let (line, color) = output::status_line(&sample, self.options.verbose);
                self.renderer.status(&line, color);
            }
        }
        self.flush_pending_ip(now);
        self.flush_pending_roam_ip(now);
        self.flush_transition_ip(now);
        self.flush_dhcp(now);
        self.flush_pending_join_csv_if_due(now);
    }

    /// A roam within 15 s of a join would otherwise discard the join's
    /// partial timing. Report it first, then start the new clock.
    fn begin_join_timing(&mut self, now: Date) {
        if let Some(timing) = self.join_timing.take_partial() {
            self.emit_join_timing(timing, now);
        }
        self.join_timing.begin(now);
    }

    /// A disconnect inside the 15 s window would otherwise discard a
    /// partial timing. On a network without IPv6 that is every join
    /// shorter than 15 s.
    fn close_join_timing(&mut self, now: Date) {
        if let Some(timing) = self.join_timing.take_partial() {
            self.emit_join_timing(timing, now);
        }
        self.join_timing.reset();
    }

    fn advance_join_timing(&mut self, sample: &Sample, now: Date) {
        if !self.join_timing.in_progress() {
            return;
        }
        if sample.status == RadioStatus::Associated {
            self.join_timing.link_up(now);
            if sample.ip_state.has_ipv4() {
                self.join_timing.ipv4_ready(now, sample.ip_state.primary_v4);
            }
            if sample.ip_state.has_global_ipv6() {
                self.join_timing.ipv6_ready(now, sample.ip_state.primary_v6);
            }
        }
        if let Some(timing) = self.join_timing.take_ready(now) {
            self.emit_join_timing(timing, now);
        }
    }

    fn emit_join_timing(&mut self, timing: JoinTiming, now: Date) {
        let line = timing.line();
        let extra: Vec<(&str, serde_json::Value)> = crate::join_timing::KEYS
            .iter()
            .filter_map(|k| timing.value(k).map(|v| (*k, json!(v))))
            .collect();
        let json = output::event_json("join_timing", &line, &now, &extra);
        self.write_event(
            &line,
            TextColor::Cyan,
            "join_timing",
            Some(json),
            false,
            now,
        );
        // The roam row waits for `IP after roam`, so the timing can still
        // ride along with it.
        if let Some(row) = self.pending_roam_csv.take() {
            self.pending_roam_csv
                .replace(row.transition, Some(timing.clone()));
        } else if let Some(row) = self.pending_join_csv.take() {
            if let Some(csv) = self.csv.as_mut() {
                csv.write_transition(&row.transition, row.source, Some(&timing), None, None);
            }
        }
    }

    fn handle_transition(
        &mut self,
        transition: &Transition,
        sample: &Sample,
        before_ip: Option<&IpState>,
    ) {
        self.last_ip_line = None;
        match transition.kind {
            TransitionKind::Roam => {
                self.roams += 1;
                self.flush_pending_roam_csv_without_ip();
                if let Some(before) = before_ip {
                    self.pending_roam_ip = Some(PendingRoamIp {
                        before: before.clone(),
                        latest: sample.ip_state.clone(),
                        start: transition.at,
                        last_change: transition.at,
                    });
                }
            }
            TransitionKind::Reconnect => self.reconnects += 1,
            TransitionKind::Disconnect => {
                self.disconnects += 1;
                self.close_join_timing(transition.at);
            }
            TransitionKind::Join => {}
        }
        self.update_history(transition, sample);
        self.flush_pending_join_csv();
        let source = sample.link.bssid_source.unwrap_or(self.bssid_source);
        let (line, color) = output::transition_line(transition);
        let json = output::transition_json(transition, source);
        self.write_event(
            &line,
            color,
            transition.kind.as_str(),
            Some(json),
            transition.kind == TransitionKind::Roam,
            transition.at,
        );
        if transition.kind == TransitionKind::Roam {
            if let Some(flushed) = self.pending_roam_csv.replace(transition.clone(), None) {
                if let Some(csv) = self.csv.as_mut() {
                    csv.write_transition(
                        &flushed.transition,
                        source,
                        flushed.timing.as_ref(),
                        None,
                        None,
                    );
                }
            }
        } else if transition.kind == TransitionKind::Disconnect {
            if let Some(csv) = self.csv.as_mut() {
                csv.write_transition(transition, source, None, None, None);
            }
        } else if self.csv.is_some() {
            self.pending_join_csv = Some(PendingJoinCsv {
                transition: transition.clone(),
                source,
                deadline: transition.at + chrono::Duration::seconds(JOIN_ROW_HOLD_SECS),
            });
        }
        if let Some(bssid) = transition.to.as_ref().and_then(|t| t.bssid.as_deref()) {
            if let Some(csv) = self.csv.as_mut() {
                csv.write_bssid(Some(bssid));
            }
        }
        if matches!(
            transition.kind,
            TransitionKind::Join | TransitionKind::Reconnect
        ) {
            self.schedule_transition_ip_line(&sample.ip_state, transition.at);
        }
    }

    fn update_history(&mut self, transition: &Transition, sample: &Sample) {
        if let Some(index) = self.current_history_index {
            if let Some(entry) = self.history.get_mut(index) {
                entry.rssi_at_leave = transition.from.as_ref().and_then(|f| f.rssi);
                entry.leave_time = Some(transition.at);
            }
        }
        let Some(new) = transition.to.as_ref() else {
            self.current_history_index = None;
            return;
        };
        self.history.push(RoamHistoryEntry {
            number: self.history.len() + 1,
            join_time: transition.at,
            ssid: new.ssid.clone(),
            bssid: new.bssid.clone().unwrap_or_default(),
            ap_name: new.ap_name.clone(),
            vendor: sample.vendor.clone(),
            channel: new.channel,
            rssi_at_join: new.rssi,
            rssi_at_leave: None,
            leave_time: None,
        });
        self.current_history_index = Some(self.history.len() - 1);
    }

    fn handle_ip_observation(&mut self, ip: &IpState, now: Date, _had_transition: bool) {
        if self.last_observed_ip.is_none() {
            self.last_observed_ip = Some(ip.clone());
            return;
        }
        if self.last_observed_ip.as_ref() == Some(ip) {
            return;
        }
        self.last_observed_ip = Some(ip.clone());
        if !ip.has_ipv4() && !ip.has_global_ipv6() && self.association.current().is_none() {
            self.pending_ip_event = None;
            self.ip_last_changed_at = None;
            return;
        }
        if self.pending_transition_ip.is_some() && (ip.has_ipv4() || ip.has_global_ipv6()) {
            self.write_ip_line(ip, now);
            self.pending_transition_ip = None;
            self.pending_ip_event = None;
            self.ip_last_changed_at = None;
            return;
        }
        self.pending_ip_event = Some(ip.clone());
        self.ip_last_changed_at = Some(now);
    }

    fn flush_pending_ip(&mut self, now: Date) {
        let (Some(ip), Some(changed)) = (self.pending_ip_event.clone(), self.ip_last_changed_at)
        else {
            return;
        };
        if time::seconds_between(&now, &changed) < 1.0 {
            return;
        }
        self.write_ip_line(&ip, now);
        self.pending_ip_event = None;
        self.ip_last_changed_at = None;
    }

    fn write_ip_line(&mut self, ip: &IpState, now: Date) {
        let line = ip.display_line();
        // IPv6 temporary-address churn changes the state without changing the
        // text. Text output skips the repeat; JSON keeps every change.
        if !self.options.json && self.last_ip_line.as_deref() == Some(line.as_str()) {
            return;
        }
        self.last_ip_line = Some(line.clone());
        let json = output::event_json("ip", &line, &now, &[("ip", ip.json_object())]);
        self.write_event(&line, TextColor::Cyan, "ip", Some(json), false, now);
    }

    fn schedule_transition_ip_line(&mut self, ip: &IpState, at: Date) {
        if ip.has_ipv4() || ip.has_global_ipv6() {
            self.write_ip_line(ip, at);
            return;
        }
        self.pending_transition_ip = Some(PendingTransitionIp {
            deadline: at + chrono::Duration::seconds(3),
            latest: ip.clone(),
        });
    }

    fn flush_transition_ip(&mut self, now: Date) {
        let Some(pending) = self.pending_transition_ip.as_ref() else {
            return;
        };
        if now < pending.deadline {
            return;
        }
        let ip = self
            .last_observed_ip
            .clone()
            .unwrap_or_else(|| pending.latest.clone());
        self.write_ip_line(&ip, now);
        self.pending_transition_ip = None;
    }

    fn flush_pending_roam_ip(&mut self, now: Date) {
        let Some(pending) = self.pending_roam_ip.as_mut() else {
            return;
        };
        if let Some(observed) = &self.last_observed_ip {
            if pending.latest != *observed {
                pending.latest = observed.clone();
                pending.last_change = now;
                return;
            }
        }
        if time::seconds_between(&now, &pending.last_change) < 1.0 {
            return;
        }
        let ms = time::millis_between(&now, &pending.start);
        let v4 = ip_state::compare_v4(&pending.before, &pending.latest, ms);
        let v6 = ip_state::compare_v6(&pending.before, &pending.latest, ms);
        let line = format!("IP after roam  {}  {}", v4.display("v4"), v6.display("v6"));
        self.pending_roam_ip = None;
        let json = output::event_json("ip", &line, &now, &[]);
        self.write_event(&line, TextColor::Cyan, "ip", Some(json), false, now);
        if let Some(row) = self.pending_roam_csv.take() {
            let source = self.bssid_source;
            // On an IPv4-only network the timing only completes at the 15 s
            // report, long after this row is written. Take what is known now.
            let timing = row.timing.or_else(|| self.join_timing.snapshot());
            if let Some(csv) = self.csv.as_mut() {
                csv.write_transition(&row.transition, source, timing.as_ref(), Some(v4), Some(v6));
            }
        }
    }

    fn flush_pending_roam_csv_without_ip(&mut self) {
        if let Some(row) = self.pending_roam_csv.take() {
            let source = self.bssid_source;
            if let Some(csv) = self.csv.as_mut() {
                csv.write_transition(&row.transition, source, row.timing.as_ref(), None, None);
            }
        }
        self.pending_roam_ip = None;
    }

    fn flush_dhcp(&mut self, now: Date) {
        if self.dhcp_deadline.is_some_and(|d| now >= d) {
            self.dhcp_deadline = None;
            let json = output::event_json("dhcp", "DHCP changed", &now, &[]);
            self.write_event(
                "DHCP changed",
                TextColor::Cyan,
                "dhcp",
                Some(json),
                false,
                now,
            );
        }
        if self.dhcpv6_deadline.is_some_and(|d| now >= d) {
            self.dhcpv6_deadline = None;
            let json = output::event_json("dhcp", "DHCPv6 changed", &now, &[]);
            self.write_event(
                "DHCPv6 changed",
                TextColor::Cyan,
                "dhcp",
                Some(json),
                false,
                now,
            );
        }
    }

    fn flush_pending_join_csv(&mut self) {
        if let Some(row) = self.pending_join_csv.take() {
            if let Some(csv) = self.csv.as_mut() {
                csv.write_transition(&row.transition, row.source, None, None, None);
            }
        }
    }

    fn flush_pending_join_csv_if_due(&mut self, now: Date) {
        if self
            .pending_join_csv
            .as_ref()
            .is_some_and(|p| now >= p.deadline)
        {
            self.flush_pending_join_csv();
        }
    }

    fn write_event(
        &mut self,
        line: &str,
        color: TextColor,
        kind: &str,
        json: Option<serde_json::Value>,
        bold: bool,
        now: Date,
    ) {
        if self.options.json {
            let value = json.unwrap_or_else(|| output::event_json(kind, line, &now, &[]));
            self.renderer.json(&value);
        } else {
            let text = format::timestamped(line, &now);
            self.renderer.event(&text, color, bold);
        }
    }

    pub fn shutdown(&mut self, now: Date) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.flush_pending_roam_csv_without_ip();
        self.flush_pending_join_csv();
        if let Some(csv) = self.csv.as_mut() {
            csv.close();
        }
        self.print_summary(now);
        if let Some(log) = &self.session_log {
            log.close();
        }
    }

    fn print_summary(&mut self, now: Date) {
        if self.options.json {
            let mut object = json!({
                "type": "summary",
                "ts": time::json(&now),
                "roams": self.roams,
                "reconnects": self.reconnects,
                "disconnects": self.disconnects,
                "distinct_bssids": self.distinct_bssids.len(),
            });
            if let Some(log) = &self.session_log {
                object["log"] = json!(log.path.display().to_string());
            }
            self.renderer.json(&object);
            return;
        }
        self.renderer.clear_status();
        self.renderer.event("Summary", TextColor::White, true);
        let line = format!(
            "elapsed {}  roams {}  reconnects {}  disconnects {}  distinct BSSIDs {}",
            units::elapsed(time::seconds_between(&now, &self.start_date)),
            self.roams,
            self.reconnects,
            self.disconnects,
            self.distinct_bssids.len()
        );
        self.renderer.event(&line, TextColor::None, false);
        let bytes = self
            .last_sample
            .as_ref()
            .map(|s| s.bytes_in.unwrap_or(0) + s.bytes_out.unwrap_or(0))
            .unwrap_or(0);
        let line = format!("bytes {}", units::data(bytes));
        self.renderer.event(&line, TextColor::None, false);
        if let Some(ip) = self.last_sample.as_ref().map(|s| s.ip_state.clone()) {
            let line = format!(
                "Final IPv4: {} gw {}",
                ip.ipv4.join(" "),
                ip.ipv4_router.as_deref().unwrap_or("none")
            );
            self.renderer.event(&line, TextColor::None, false);
            let v6: Vec<String> = ip
                .ipv6
                .iter()
                .map(|a| {
                    let prefix = a.prefix.map(|p| format!("/{p}")).unwrap_or_default();
                    format!("{}{} {}", a.address, prefix, a.kind().as_str())
                })
                .collect();
            let line = format!(
                "Final IPv6: {} gw {}",
                v6.join(" "),
                ip.ipv6_router.as_deref().unwrap_or("none")
            );
            self.renderer.event(&line, TextColor::None, false);
        }
        if !self.history.is_empty() {
            self.renderer
                .event(RoamHistoryEntry::HEADER, TextColor::None, false);
            let lines: Vec<String> = self.history.iter().map(|e| e.line(&now)).collect();
            for line in lines {
                self.renderer.event(&line, TextColor::None, false);
            }
        }
        if let Some(log) = &self.session_log {
            let line = format!("log {}", log.path.display());
            self.renderer.event(&line, TextColor::None, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ie::SecurityInfo;
    use crate::ip_state::{Ipv6AddressInfo, Ipv6Origin};
    use crate::link::LinkSample;
    use chrono::{Duration, Local, TimeZone};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct Buffer(Rc<RefCell<String>>);

    impl Sink for Buffer {
        fn write_raw(&mut self, text: &str) {
            self.0.borrow_mut().push_str(text);
        }
        fn is_tty(&self) -> bool {
            false
        }
        fn columns(&self) -> Option<usize> {
            None
        }
    }

    fn at(s: i64) -> Date {
        Local.with_ymd_and_hms(2026, 10, 5, 9, 0, 0).unwrap() + Duration::milliseconds(s)
    }

    fn monitor(json: bool, verbose: bool) -> (Monitor<Buffer>, Buffer) {
        let buffer = Buffer::default();
        let options = RunOptions {
            json,
            no_color: true,
            verbose,
            ..Default::default()
        };
        let mut map = std::collections::HashMap::new();
        map.insert("00:0B:86".to_string(), "Aruba".to_string());
        let config = MonitorConfig {
            options,
            interface_name: "Wi-Fi".into(),
            os_line: "Windows 11 build 26100".into(),
            oui: Oui::from_map(map),
            oui_warning: None,
            session_log: None,
            session_log_warning: None,
        };
        let mut m = Monitor::new(config, buffer.clone(), at(0));
        m.start(at(0));
        (m, buffer)
    }

    fn link(bssid: &str, ch: u32, rssi: i32) -> LinkState {
        LinkState::associated(LinkSample {
            ssid: Some("Corp".into()),
            bssid: Some(bssid.into()),
            bssid_source: Some(BssidSource::Bss),
            ap_name: Some(format!("ap-{}", &bssid[15..])),
            channel: Some(ch),
            width_mhz: Some(80),
            band: Some("5".into()),
            phy: Some("802.11ax".into()),
            security: Some(SecurityInfo {
                security: Some("wpa2-enterprise".into()),
                ft: true,
                ..Default::default()
            }),
            tx_rate_mbps: Some(1200.0),
            rssi_dbm: Some(rssi),
            ..Default::default()
        })
    }

    fn ip(v4: &str, v6: &str) -> IpState {
        IpState {
            ipv4: vec![v4.into()],
            ipv4_prefixes: vec![Some(24)],
            ipv4_router: Some("192.0.2.1".into()),
            ipv6: vec![Ipv6AddressInfo::new(
                v6,
                Some(64),
                Ipv6Origin {
                    prefix_origin: 4,
                    suffix_origin: 4,
                    dad_state: 4,
                    expired: false,
                },
            )],
            ipv6_router: Some("fe80::1".into()),
            primary_v4: true,
            primary_v6: true,
        }
    }

    fn reading(link: LinkState, ip: IpState, bytes: u64) -> Reading {
        Reading {
            link,
            ip,
            counters: Some(CounterSample {
                bytes_in: bytes,
                bytes_out: 0,
            }),
            bss_denied: false,
        }
    }

    #[test]
    fn join_roam_ip_after_roam_and_summary() {
        let (mut m, out) = monitor(false, false);
        m.sample(
            reading(
                link("00:0b:86:11:22:01", 36, -55),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Startup,
            at(1000),
        );
        m.sample(
            reading(
                link("00:0b:86:11:22:01", 36, -60),
                ip("192.0.2.143", "2001:db8:1::1234"),
                1_000_000,
            ),
            SampleReason::Poll,
            at(2000),
        );
        m.sample(
            reading(
                link("00:0b:86:11:22:02", 44, -58),
                ip("192.0.2.143", "2001:db8:1::1234"),
                2_000_000,
            ),
            SampleReason::Poll,
            at(15_000),
        );
        m.sample(
            reading(
                link("00:0b:86:11:22:02", 44, -58),
                ip("192.0.2.143", "2001:db8:1::5678"),
                3_000_000,
            ),
            SampleReason::Settle,
            at(15_500),
        );
        m.sample(
            reading(
                link("00:0b:86:11:22:02", 44, -58),
                ip("192.0.2.143", "2001:db8:1::5678"),
                3_000_000,
            ),
            SampleReason::Poll,
            at(17_100),
        );
        m.sample(
            reading(LinkState::disconnected(), IpState::default(), 3_000_000),
            SampleReason::Poll,
            at(20_000),
        );
        m.sample(
            reading(
                link("00:0b:86:11:22:02", 44, -61),
                ip("192.0.2.143", "2001:db8:1::5678"),
                3_000_000,
            ),
            SampleReason::Poll,
            at(25_000),
        );
        m.shutdown(at(30_000));
        let text = out.0.borrow().clone();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            format!("iairport v{VERSION} (Windows port of iAirport by Guillaume Germain)")
        );
        assert_eq!(lines[1], "Windows 11 build 26100");
        assert_eq!(lines[2], "interface Wi-Fi");
        assert_eq!(
            lines[3],
            "2026/10/05 09:00:01  JOIN  00:0b:86:11:22:01 (ap-01)  \"Corp\"  ch 36  RSSI -55"
        );
        assert_eq!(lines[4], "2026/10/05 09:00:01  IP  v4 192.0.2.143/24 gw 192.0.2.1  |  v6 2001:db8:1::1234/64 slaac gw fe80::1");
        assert_eq!(lines[5], "2026/10/05 09:00:01  \"Corp\" (Aruba 00:0b:86:11:22:01 ap-01) Chan 36/80 802.11ax wpa2-enterprise FT (1200 Mbps)  RSSI -55  Speed: 0bps Data: 0");
        assert_eq!(lines[6], "2026/10/05 09:00:02  \"Corp\" (Aruba 00:0b:86:11:22:01 ap-01) Chan 36/80 802.11ax wpa2-enterprise FT (1200 Mbps)  RSSI -60  Speed: 8.0Mbps Data: 1.0M");
        assert_eq!(lines[7], "2026/10/05 09:00:15  ROAM  00:0b:86:11:22:01 (ap-01) -> 00:0b:86:11:22:02 (ap-02)  \"Corp\"  ch 36 -> 44  RSSI -60 -> -58");
        assert!(
            lines[8].starts_with("2026/10/05 09:00:15  \"Corp\" (Aruba 00:0b:86:11:22:02 ap-02)")
        );
        assert!(
            lines[9].starts_with("2026/10/05 09:00:17  \"Corp\" (Aruba 00:0b:86:11:22:02 ap-02)")
        );
        assert_eq!(lines[10], "2026/10/05 09:00:17  IP  v4 192.0.2.143/24 gw 192.0.2.1  |  v6 2001:db8:1::5678/64 slaac gw fe80::1");
        assert_eq!(
            lines[11],
            "2026/10/05 09:00:17  IP after roam  v4 kept  v6 changed 2.1 s"
        );
        assert_eq!(
            lines[12],
            "2026/10/05 09:00:20  DISCONNECT  00:0b:86:11:22:02 (ap-02)  \"Corp\""
        );
        assert_eq!(lines[13], "2026/10/05 09:00:20  Not connected");
        assert_eq!(
            lines[14],
            "2026/10/05 09:00:25  RECONNECT  00:0b:86:11:22:02 (ap-02)  \"Corp\""
        );
        assert_eq!(lines[15], "2026/10/05 09:00:25  IP  v4 192.0.2.143/24 gw 192.0.2.1  |  v6 2001:db8:1::5678/64 slaac gw fe80::1");
        let summary = lines.iter().position(|l| *l == "Summary").unwrap();
        assert_eq!(
            lines[summary + 1],
            "elapsed 30s  roams 1  reconnects 1  disconnects 1  distinct BSSIDs 2"
        );
        assert_eq!(lines[summary + 2], "bytes 2.9M");
        assert_eq!(lines[summary + 3], "Final IPv4: 192.0.2.143 gw 192.0.2.1");
        assert_eq!(
            lines[summary + 4],
            "Final IPv6: 2001:db8:1::5678/64 slaac gw fe80::1"
        );
        assert_eq!(lines[summary + 5], RoamHistoryEntry::HEADER);
        assert_eq!(
            lines[summary + 6],
            " 1  2026/10/05 09:00:01  Corp  00:0b:86:11:22:01  ap-01  Aruba  36  -55  -60  14s"
        );
        assert_eq!(
            lines[summary + 7],
            " 2  2026/10/05 09:00:15  Corp  00:0b:86:11:22:02  ap-02  Aruba  44  -58  -58  5s"
        );
        assert_eq!(
            lines[summary + 8],
            " 3  2026/10/05 09:00:25  Corp  00:0b:86:11:22:02  ap-02  Aruba  44  -61    5s"
        );
    }

    #[test]
    fn json_mode_emits_objects_and_summary() {
        let (mut m, out) = monitor(true, false);
        m.sample(
            reading(
                link("00:0b:86:11:22:01", 36, -55),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Startup,
            at(1000),
        );
        m.shutdown(at(5000));
        let text = out.0.borrow().clone();
        let objects: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(objects[0]["type"], "join");
        assert_eq!(objects[1]["type"], "ip");
        assert_eq!(objects[1]["ip"]["v4"][0], "192.0.2.143");
        assert_eq!(objects[2]["type"], "sample");
        assert_eq!(objects[2]["bssid_source"], "bss");
        let summary = objects.last().unwrap();
        let keys: Vec<&str> = summary
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "disconnects",
                "distinct_bssids",
                "reconnects",
                "roams",
                "ts",
                "type"
            ]
        );
    }

    #[test]
    fn join_timing_from_wlan_events() {
        let (mut m, out) = monitor(false, false);
        m.on_wlan_event(WlanEvent::Associating, at(0));
        m.on_wlan_event(WlanEvent::Associated { bssid: None }, at(272));
        m.on_wlan_event(WlanEvent::Connected { bssid: None }, at(1220));
        assert!(m.next_deadline().is_some());
        m.sample(
            reading(link("00:0b:86:11:22:01", 36, -55), IpState::default(), 0),
            SampleReason::Settle,
            at(1501),
        );
        m.sample(
            reading(
                link("00:0b:86:11:22:01", 36, -55),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Poll,
            at(2556),
        );
        let text = out.0.borrow().clone();
        assert!(text.contains("2026/10/05 09:00:02  JOIN TIMING  assoc 272 ms  auth 948 ms  linkup 281 ms  ipv4 1336 ms  ipv6 1336 ms  ipv4Primary 1336 ms  ipv6Primary 1336 ms\n"), "{text}");
        assert!(text.contains("JOIN  00:0b:86:11:22:01"));
    }

    #[test]
    fn join_timing_assoc_starts_at_connection_start() {
        let (mut m, out) = monitor(false, false);
        m.on_wlan_event(WlanEvent::ConnectionStart, at(0));
        // Windows reports associating and associated in the same instant.
        m.on_wlan_event(WlanEvent::Associating, at(400));
        m.on_wlan_event(WlanEvent::Associated { bssid: None }, at(400));
        m.on_wlan_event(WlanEvent::Connected { bssid: None }, at(900));
        m.sample(
            reading(
                link("00:0b:86:11:22:01", 36, -55),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Settle,
            at(1000),
        );
        let text = out.0.borrow().clone();
        assert!(
            text.contains("JOIN TIMING  assoc 400 ms  auth 500 ms  linkup 100 ms"),
            "{text}"
        );
    }

    /// The poll prints JOIN before the timing is known. The roams.csv row
    /// must wait for its own timing and never take the next join's.
    #[test]
    fn join_row_waits_for_its_timing() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("join-row-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut m, _out) = monitor(false, false);
        let mut warnings = Vec::new();
        m.set_csv(CsvLogger::open(&dir, &at(0), &mut warnings));
        let roams = || std::fs::read_to_string(dir.join("iairport-roams.csv")).unwrap();

        m.on_wlan_event(WlanEvent::ConnectionStart, at(0));
        m.on_wlan_event(WlanEvent::Associated { bssid: None }, at(300));
        m.on_wlan_event(WlanEvent::Connected { bssid: None }, at(800));
        m.sample(
            reading(link("00:0b:86:11:22:01", 36, -55), IpState::default(), 0),
            SampleReason::Settle,
            at(1000),
        );
        assert_eq!(roams().lines().count(), 1, "row written before timing");
        m.sample(
            reading(
                link("00:0b:86:11:22:01", 36, -55),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Poll,
            at(2000),
        );
        let text = roams();
        let row = text.lines().nth(1).unwrap();
        assert!(row.contains(",join,Corp,,00:0b:86:11:22:01,"), "{row}");
        assert!(row.ends_with(",300,500,200,1200,1200,1200,1200"), "{row}");

        // Second join with no notifications at all: the row goes out at the
        // hold deadline with empty timing columns.
        m.sample(
            reading(LinkState::disconnected(), IpState::default(), 0),
            SampleReason::Poll,
            at(5000),
        );
        m.sample(
            reading(
                link("00:0b:86:11:22:02", 44, -60),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Poll,
            at(6000),
        );
        assert_eq!(roams().lines().count(), 3, "join row held");
        m.sample(
            reading(
                link("00:0b:86:11:22:02", 44, -60),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Poll,
            at(6000 + JOIN_ROW_HOLD_SECS * 1000),
        );
        let text = roams();
        assert_eq!(text.lines().count(), 4);
        let row = text.lines().nth(3).unwrap();
        assert!(row.contains(",join,Corp,,00:0b:86:11:22:02,"), "{row}");
        assert!(row.ends_with(",,,,,,,,"), "{row}");
        m.shutdown(at(30_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ACM-only mode: no MSM notifications. The clock anchors on connection
    /// complete, so linkup and the IP phases still print, without assoc or
    /// auth.
    #[test]
    fn join_timing_without_msm_uses_connection_complete() {
        let (mut m, out) = monitor(false, false);
        m.on_wlan_event(WlanEvent::ConnectionStart, at(0));
        m.on_wlan_event(WlanEvent::ConnectionComplete, at(900));
        m.sample(
            reading(
                link("00:0b:86:11:22:01", 36, -55),
                ip("192.0.2.143", "2001:db8:1::1234"),
                0,
            ),
            SampleReason::Settle,
            at(1000),
        );
        let text = out.0.borrow().clone();
        assert!(
            text.contains("JOIN TIMING  linkup 100 ms  ipv4 100 ms  ipv6 100 ms"),
            "{text}"
        );
        assert!(!text.contains("assoc"), "{text}");
    }

    /// A join that never gets past connection start closes at the report
    /// deadline, so the monitor returns to the normal poll interval.
    #[test]
    fn join_timing_with_nothing_known_closes_at_deadline() {
        let (mut m, out) = monitor(false, false);
        m.on_wlan_event(WlanEvent::RoamingStart, at(0));
        m.on_wlan_event(WlanEvent::RoamingEnd, at(50));
        let link_state = link("00:0b:86:11:22:01", 36, -55);
        let ip_state = ip("192.0.2.143", "2001:db8:1::1234");
        m.sample(
            reading(link_state.clone(), ip_state.clone(), 0),
            SampleReason::Settle,
            at(1000),
        );
        assert!(m.join_timing.in_progress());
        m.sample(
            reading(link_state, ip_state, 0),
            SampleReason::Poll,
            at(1000 + crate::join_timing::REPORT_AFTER_MS),
        );
        assert!(!m.join_timing.in_progress());
        assert!(!out.0.borrow().contains("JOIN TIMING"));
    }

    /// A short-lived join on a network without IPv6 reports its partial
    /// timing at the disconnect instead of losing it.
    #[test]
    fn disconnect_reports_partial_join_timing() {
        let (mut m, out) = monitor(false, false);
        let mut v4_only = ip("192.0.2.143", "2001:db8:1::1234");
        v4_only.ipv6.clear();
        v4_only.ipv6_router = None;
        m.on_wlan_event(WlanEvent::ConnectionStart, at(0));
        m.on_wlan_event(WlanEvent::Associated { bssid: None }, at(300));
        m.on_wlan_event(WlanEvent::Connected { bssid: None }, at(800));
        m.sample(
            reading(link("00:0b:86:11:22:01", 36, -55), v4_only, 0),
            SampleReason::Settle,
            at(1000),
        );
        assert!(!out.0.borrow().contains("JOIN TIMING"));
        m.sample(
            reading(LinkState::disconnected(), IpState::default(), 0),
            SampleReason::Poll,
            at(6000),
        );
        let text = out.0.borrow().clone();
        assert!(
            text.contains("JOIN TIMING  assoc 300 ms  auth 500 ms  linkup 200 ms  ipv4 200 ms  ipv4Primary 200 ms"),
            "{text}"
        );
        assert!(!m.join_timing.in_progress());
    }

    /// IPv4-only network: the roam row is written about a second after the
    /// roam, before the 15 s partial report. It carries the phases known at
    /// that moment. A roam soon after a join reports the join first.
    #[test]
    fn roam_row_carries_partial_timing_on_ipv4_only_network() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("roam-row-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut m, out) = monitor(false, false);
        let mut warnings = Vec::new();
        m.set_csv(CsvLogger::open(&dir, &at(0), &mut warnings));
        let roams = || std::fs::read_to_string(dir.join("iairport-roams.csv")).unwrap();
        let mut v4_only = ip("192.0.2.143", "2001:db8:1::1234");
        v4_only.ipv6.clear();
        v4_only.ipv6_router = None;

        m.on_wlan_event(WlanEvent::ConnectionStart, at(0));
        m.on_wlan_event(WlanEvent::Associated { bssid: None }, at(300));
        m.on_wlan_event(WlanEvent::Connected { bssid: None }, at(800));
        m.sample(
            reading(link("00:0b:86:11:22:01", 36, -55), v4_only.clone(), 0),
            SampleReason::Settle,
            at(1000),
        );
        // Roam 5 s later, before the join's timing was reported.
        m.on_wlan_event(WlanEvent::RoamingStart, at(5000));
        m.on_wlan_event(WlanEvent::Associated { bssid: None }, at(5020));
        m.on_wlan_event(WlanEvent::Connected { bssid: None }, at(5050));
        m.sample(
            reading(link("00:0b:86:11:22:02", 44, -60), v4_only.clone(), 0),
            SampleReason::Settle,
            at(5100),
        );
        m.sample(
            reading(link("00:0b:86:11:22:02", 44, -60), v4_only, 0),
            SampleReason::Poll,
            at(6200),
        );
        let text = out.0.borrow().clone();
        assert!(
            text.contains("JOIN TIMING  assoc 300 ms  auth 500 ms  linkup 200 ms  ipv4 200 ms  ipv4Primary 200 ms"),
            "{text}"
        );
        assert!(text.contains("IP after roam  v4 kept  v6 kept"), "{text}");
        let csv = roams();
        assert_eq!(csv.lines().count(), 3, "{csv}");
        let join = csv.lines().nth(1).unwrap();
        assert!(join.ends_with(",300,500,200,200,,200,"), "{join}");
        let roam = csv.lines().nth(2).unwrap();
        assert!(
            roam.contains(",roam,Corp,00:0b:86:11:22:01,00:0b:86:11:22:02,"),
            "{roam}"
        );
        assert!(roam.ends_with(",20,30,50,50,,50,"), "{roam}");
        m.shutdown(at(30_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wlan_reason_and_deauth_lines() {
        let (mut m, out) = monitor(false, true);
        m.on_wlan_event(
            WlanEvent::Disconnected {
                reason: 163841,
                reason_text: "The network is disconnected by the driver.".into(),
            },
            at(1000),
        );
        m.on_log_event(
            LogEvent::Deauth {
                kind: "DEAUTH".into(),
                source: None,
                reason: 3,
            },
            at(2000),
        );
        m.on_log_event(
            LogEvent::Verbose("802.11 security started".into()),
            at(2500),
        );
        m.on_log_event(LogEvent::Dhcp, at(3000));
        m.sample(
            reading(LinkState::disconnected(), IpState::default(), 0),
            SampleReason::Poll,
            at(5000),
        );
        let text = out.0.borrow().clone();
        assert!(text.contains("2026/10/05 09:00:01  WLAN DISCONNECTED  reason 163841 The network is disconnected by the driver.\n"));
        assert!(text.contains("2026/10/05 09:00:02  DEAUTH  reason 3 deauthenticated because station is leaving (could be a ClientMatch move)\n"));
        assert!(text.contains("2026/10/05 09:00:02  802.11 security started\n"));
        assert!(text.contains("2026/10/05 09:00:05  DHCP changed\n"));
        assert_eq!(m.settle_times.len(), 0);
    }

    #[test]
    fn location_hint_prints_once_and_signal_pct_shows() {
        let (mut m, out) = monitor(false, false);
        let mut l = link("00:0b:86:11:22:01", 36, -55);
        l.link.rssi_dbm = None;
        l.link.ap_name = None;
        l.link.bssid_source = Some(BssidSource::Connection);
        l.link.signal_pct = Some(84);
        let mut r = reading(l, ip("192.0.2.143", "2001:db8:1::1234"), 0);
        r.bss_denied = true;
        m.sample(r.clone(), SampleReason::Startup, at(1000));
        m.sample(r, SampleReason::Poll, at(2000));
        let text = out.0.borrow().clone();
        assert_eq!(text.matches("Wi-Fi details withheld").count(), 1);
        assert!(text.contains("(Aruba 00:0b:86:11:22:01) Chan 36/80 802.11ax wpa2-enterprise FT (1200 Mbps)  Signal 84%  Speed:"));
        assert!(text.contains("JOIN  00:0b:86:11:22:01  \"Corp\"  ch 36  RSSI \n"));
    }

    #[test]
    fn settle_reads_schedule_and_expire() {
        let (mut m, _) = monitor(false, false);
        m.on_wlan_event(WlanEvent::RoamingEnd, at(1000));
        assert_eq!(m.settle_times.len(), 4);
        assert_eq!(m.next_deadline(), Some(at(1000)));
        m.sample(
            reading(LinkState::disconnected(), IpState::default(), 0),
            SampleReason::Settle,
            at(1000),
        );
        assert_eq!(m.next_deadline(), Some(at(1500)));
        m.sample(
            reading(LinkState::disconnected(), IpState::default(), 0),
            SampleReason::Settle,
            at(4000),
        );
        assert_eq!(m.next_deadline(), None);
    }
}
