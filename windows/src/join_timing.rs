use crate::Date;

pub const KEYS: [&str; 7] = [
    "assoc",
    "auth",
    "linkup",
    "ipv4",
    "ipv6",
    "ipv4Primary",
    "ipv6Primary",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JoinTiming {
    pub assoc: Option<i64>,
    pub auth: Option<i64>,
    pub linkup: Option<i64>,
    pub ipv4: Option<i64>,
    pub ipv6: Option<i64>,
    pub ipv4_primary: Option<i64>,
    pub ipv6_primary: Option<i64>,
    pub start: Option<Date>,
}

impl JoinTiming {
    pub fn value(&self, key: &str) -> Option<i64> {
        match key {
            "assoc" => self.assoc,
            "auth" => self.auth,
            "linkup" => self.linkup,
            "ipv4" => self.ipv4,
            "ipv6" => self.ipv6,
            "ipv4Primary" => self.ipv4_primary,
            "ipv6Primary" => self.ipv6_primary,
            _ => None,
        }
    }

    pub fn is_complete(&self) -> bool {
        ["assoc", "auth", "linkup", "ipv4", "ipv6"]
            .iter()
            .all(|k| self.value(k).is_some())
    }

    pub fn has_any(&self) -> bool {
        KEYS.iter().any(|k| self.value(k).is_some())
    }

    pub fn line(&self) -> String {
        let parts: Vec<String> = KEYS
            .iter()
            .filter_map(|k| self.value(k).map(|v| format!("{k} {v} ms")))
            .collect();
        format!("JOIN TIMING  {}", parts.join("  "))
    }
}

/// Builds join timing from wlanapi notification timestamps. On Windows the
/// phases are: assoc = ACM connection start (or MSM roaming start) to MSM
/// associated, auth = associated to connected (MSM), linkup = connected to
/// the first sample whose interface state is connected, ipv4/ipv6 =
/// connected to the first usable address. MSM associating and associated
/// arrive in the same instant on the drivers seen so far, so associating
/// only starts the clock when no connection start preceded it.
#[derive(Default)]
pub struct JoinTimingTracker {
    associating: Option<Date>,
    associated: Option<Date>,
    connected: Option<Date>,
    timing: JoinTiming,
    reported: bool,
    /// Set when ACM connection complete anchored the clock with no MSM
    /// notification before it. `assoc` and `auth` cannot arrive then.
    without_msm: bool,
}

/// Time after the start of a join when a partial timing line is printed.
pub const REPORT_AFTER_MS: i64 = 15_000;

impl JoinTimingTracker {
    /// Starts a new join at a connection start or roaming start.
    pub fn begin(&mut self, now: Date) {
        *self = JoinTimingTracker::default();
        self.associating = Some(now);
        self.timing.start = Some(now);
    }

    pub fn associating(&mut self, now: Date) {
        let open = self.associating.is_some() && self.associated.is_none() && !self.reported;
        if !open {
            self.begin(now);
        }
    }

    pub fn associated(&mut self, now: Date) {
        if self.associating.is_none() {
            self.associating(now);
        }
        if self.associated.is_none() {
            self.associated = Some(now);
            self.timing.assoc = self.associating.map(|t| ms(t, now));
        }
    }

    /// MSM connected. Ignored when no join is open: the Intel driver
    /// re-emits connected for the current BSSID at the moment of a
    /// user-requested disconnect, and that must not start a clock.
    pub fn connected(&mut self, now: Date) {
        if self.timing.start.is_none() || self.reported {
            return;
        }
        if self.associated.is_none() {
            self.associated(now);
        }
        if self.connected.is_none() {
            self.connected = Some(now);
            self.timing.auth = self.associated.map(|t| ms(t, now));
        }
    }

    /// ACM connection complete. Anchors the clock when MSM notifications are
    /// refused, so linkup and the IP phases can still be measured. `assoc`
    /// and `auth` stay blank unless MSM reported them.
    pub fn connection_complete(&mut self, now: Date) {
        if self.timing.start.is_none() || self.connected.is_some() {
            return;
        }
        self.connected = Some(now);
        match self.associated {
            Some(associated) => self.timing.auth = Some(ms(associated, now)),
            None => self.without_msm = true,
        }
    }

    fn complete(&self) -> bool {
        if self.without_msm {
            return ["linkup", "ipv4", "ipv6"]
                .iter()
                .all(|k| self.timing.value(k).is_some());
        }
        self.timing.is_complete()
    }

    pub fn link_up(&mut self, now: Date) {
        if let (Some(connected), None) = (self.connected, self.timing.linkup) {
            self.timing.linkup = Some(ms(connected, now));
        }
    }

    pub fn ipv4_ready(&mut self, now: Date, primary: bool) {
        if let Some(connected) = self.connected {
            if self.timing.ipv4.is_none() {
                self.timing.ipv4 = Some(ms(connected, now));
            }
            if primary && self.timing.ipv4_primary.is_none() {
                self.timing.ipv4_primary = Some(ms(connected, now));
            }
        }
    }

    pub fn ipv6_ready(&mut self, now: Date, primary: bool) {
        if let Some(connected) = self.connected {
            if self.timing.ipv6.is_none() {
                self.timing.ipv6 = Some(ms(connected, now));
            }
            if primary && self.timing.ipv6_primary.is_none() {
                self.timing.ipv6_primary = Some(ms(connected, now));
            }
        }
    }

    pub fn in_progress(&self) -> bool {
        self.timing.start.is_some() && !self.reported
    }

    /// Returns the timing once: when complete, or when the join is older
    /// than `REPORT_AFTER_MS` and at least one phase is known. A join that
    /// old with nothing known is closed silently, so the tracker cannot
    /// hold the monitor at the 1 s settle rate for the whole connection.
    pub fn take_ready(&mut self, now: Date) -> Option<JoinTiming> {
        if self.reported {
            return None;
        }
        let start = self.timing.start?;
        let age = ms(start, now);
        if self.complete() || (age >= REPORT_AFTER_MS && self.timing.has_any()) {
            self.reported = true;
            return Some(self.timing.clone());
        }
        if age >= REPORT_AFTER_MS {
            self.reported = true;
        }
        None
    }

    /// The phases known so far for a join still in progress.
    pub fn snapshot(&self) -> Option<JoinTiming> {
        (self.in_progress() && self.timing.has_any()).then(|| self.timing.clone())
    }

    /// Closes a join in progress and returns what was known, so a new join
    /// can start without losing the previous one.
    pub fn take_partial(&mut self) -> Option<JoinTiming> {
        let timing = self.snapshot()?;
        self.reported = true;
        Some(timing)
    }

    pub fn reset(&mut self) {
        *self = JoinTimingTracker::default();
    }
}

fn ms(from: Date, to: Date) -> i64 {
    (to - from).num_milliseconds().max(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local};

    #[test]
    fn line_matches_docs() {
        let t = JoinTiming {
            assoc: Some(272),
            auth: Some(948),
            linkup: Some(281),
            ipv4: Some(1336),
            ipv6: Some(1542),
            ..Default::default()
        };
        assert!(t.is_complete());
        assert_eq!(
            t.line(),
            "JOIN TIMING  assoc 272 ms  auth 948 ms  linkup 281 ms  ipv4 1336 ms  ipv6 1542 ms"
        );
        let partial = JoinTiming {
            assoc: Some(10),
            ipv6_primary: Some(99),
            ..Default::default()
        };
        assert_eq!(
            partial.line(),
            "JOIN TIMING  assoc 10 ms  ipv6Primary 99 ms"
        );
    }

    #[test]
    fn tracker_phases() {
        let t0 = Local::now();
        let mut tr = JoinTimingTracker::default();
        tr.associating(t0);
        tr.associated(t0 + Duration::milliseconds(272));
        tr.connected(t0 + Duration::milliseconds(1220));
        assert!(tr.take_ready(t0 + Duration::milliseconds(1300)).is_none());
        tr.link_up(t0 + Duration::milliseconds(1501));
        tr.ipv4_ready(t0 + Duration::milliseconds(2556), true);
        tr.ipv6_ready(t0 + Duration::milliseconds(2762), false);
        let timing = tr.take_ready(t0 + Duration::milliseconds(2762)).unwrap();
        assert_eq!(
            timing.line(),
            "JOIN TIMING  assoc 272 ms  auth 948 ms  linkup 281 ms  ipv4 1336 ms  ipv6 1542 ms  ipv4Primary 1336 ms"
        );
        assert!(tr.take_ready(t0 + Duration::seconds(5)).is_none());
    }

    #[test]
    fn partial_report_after_deadline() {
        let t0 = Local::now();
        let mut tr = JoinTimingTracker::default();
        tr.associating(t0);
        tr.associated(t0 + Duration::milliseconds(100));
        tr.connected(t0 + Duration::milliseconds(400));
        assert!(tr.take_ready(t0 + Duration::seconds(10)).is_none());
        let timing = tr.take_ready(t0 + Duration::seconds(16)).unwrap();
        assert_eq!(timing.line(), "JOIN TIMING  assoc 100 ms  auth 300 ms");
        assert!(!tr.in_progress());
    }

    #[test]
    fn connected_without_an_open_join_is_ignored() {
        let t0 = Local::now();
        let mut tr = JoinTimingTracker::default();
        tr.connected(t0);
        assert!(!tr.in_progress());
        assert!(tr.snapshot().is_none());

        // Once a join is open, connected backfills the missing phases.
        tr.begin(t0);
        tr.connected(t0 + Duration::milliseconds(400));
        assert_eq!(tr.timing.assoc, Some(400));
        assert_eq!(tr.timing.auth, Some(0));
    }
}
