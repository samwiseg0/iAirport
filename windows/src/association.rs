use crate::link::{LinkState, RadioStatus};
use crate::Date;

#[derive(Clone, Debug, PartialEq)]
pub struct Association {
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub ap_name: Option<String>,
    pub channel: Option<u32>,
    pub rssi: Option<i32>,
    pub since: Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionKind {
    Join,
    Roam,
    Reconnect,
    Disconnect,
}

impl TransitionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransitionKind::Join => "join",
            TransitionKind::Roam => "roam",
            TransitionKind::Reconnect => "reconnect",
            TransitionKind::Disconnect => "disconnect",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub kind: TransitionKind,
    pub from: Option<Association>,
    pub to: Option<Association>,
    pub dwell_seconds: Option<f64>,
    pub at: Date,
}

#[derive(Default)]
pub struct AssociationTracker {
    current: Option<Association>,
    last_associated: Option<Association>,
    current_status: Option<RadioStatus>,
    pub generation: u64,
}

impl AssociationTracker {
    pub fn current(&self) -> Option<&Association> {
        self.current.as_ref()
    }

    pub fn last_associated(&self) -> Option<&Association> {
        self.last_associated.as_ref()
    }

    pub fn status(&self) -> Option<RadioStatus> {
        self.current_status
    }

    pub fn update_rssi(&mut self, rssi: Option<i32>) {
        if let Some(cur) = self.current.as_mut() {
            if rssi.is_some() {
                cur.rssi = rssi;
            }
        }
    }

    pub fn commit(&mut self, state: &LinkState, now: Date) -> Option<Transition> {
        let previous_status = self.current_status;
        self.current_status = Some(state.status);
        match state.status {
            RadioStatus::Associated => {
                let mut incoming = Association {
                    ssid: state.link.ssid.clone(),
                    bssid: state.link.bssid.clone(),
                    ap_name: state.link.ap_name.clone(),
                    channel: state.link.channel,
                    rssi: state.link.rssi_dbm,
                    since: now,
                };
                match self.current.take() {
                    // A missing BSSID on either side means Windows withheld
                    // it (Location off), so it is the same association.
                    Some(cur)
                        if cur.bssid == incoming.bssid
                            || cur.bssid.is_none()
                            || incoming.bssid.is_none() =>
                    {
                        incoming.since = cur.since;
                        if incoming.bssid.is_none() {
                            incoming.bssid = cur.bssid.clone();
                        }
                        if incoming.ssid.is_none() {
                            incoming.ssid = cur.ssid.clone();
                        }
                        if incoming.ap_name.is_none() {
                            incoming.ap_name = cur.ap_name.clone();
                        }
                        self.current = Some(incoming.clone());
                        self.last_associated = Some(incoming);
                        None
                    }
                    Some(cur) => {
                        let dwell = (now - cur.since).num_milliseconds() as f64 / 1000.0;
                        self.generation += 1;
                        self.current = Some(incoming.clone());
                        self.last_associated = Some(incoming.clone());
                        Some(Transition {
                            kind: TransitionKind::Roam,
                            from: Some(cur),
                            to: Some(incoming),
                            dwell_seconds: Some(dwell),
                            at: now,
                        })
                    }
                    None => {
                        let kind = match (&self.last_associated, previous_status) {
                            (Some(last), Some(_))
                                if last.bssid.is_some() && last.bssid == incoming.bssid =>
                            {
                                TransitionKind::Reconnect
                            }
                            _ => TransitionKind::Join,
                        };
                        let from = if kind == TransitionKind::Reconnect {
                            self.last_associated.clone()
                        } else {
                            None
                        };
                        self.generation += 1;
                        self.current = Some(incoming.clone());
                        self.last_associated = Some(incoming.clone());
                        Some(Transition {
                            kind,
                            from,
                            to: Some(incoming),
                            dwell_seconds: None,
                            at: now,
                        })
                    }
                }
            }
            RadioStatus::Disconnected | RadioStatus::Off => match self.current.take() {
                Some(cur) => {
                    let dwell = (now - cur.since).num_milliseconds() as f64 / 1000.0;
                    self.generation += 1;
                    self.last_associated = Some(cur.clone());
                    Some(Transition {
                        kind: TransitionKind::Disconnect,
                        from: Some(cur),
                        to: None,
                        dwell_seconds: Some(dwell),
                        at: now,
                    })
                }
                None => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::LinkSample;
    use chrono::{Duration, Local};

    fn assoc(bssid: &str) -> LinkState {
        LinkState::associated(LinkSample {
            ssid: Some("Corp".into()),
            bssid: Some(bssid.into()),
            channel: Some(36),
            rssi_dbm: Some(-55),
            ..Default::default()
        })
    }

    #[test]
    fn join_roam_disconnect_reconnect() {
        let mut t = AssociationTracker::default();
        let t0 = Local::now();
        let first = t.commit(&assoc("00:0b:86:11:22:01"), t0).unwrap();
        assert_eq!(first.kind, TransitionKind::Join);
        assert!(t
            .commit(&assoc("00:0b:86:11:22:01"), t0 + Duration::seconds(1))
            .is_none());
        let roam = t
            .commit(&assoc("00:0b:86:11:22:02"), t0 + Duration::seconds(10))
            .unwrap();
        assert_eq!(roam.kind, TransitionKind::Roam);
        assert_eq!(
            roam.from.as_ref().unwrap().bssid.as_deref(),
            Some("00:0b:86:11:22:01")
        );
        assert!((roam.dwell_seconds.unwrap() - 10.0).abs() < 0.01);
        let disc = t
            .commit(&LinkState::disconnected(), t0 + Duration::seconds(15))
            .unwrap();
        assert_eq!(disc.kind, TransitionKind::Disconnect);
        assert!((disc.dwell_seconds.unwrap() - 5.0).abs() < 0.01);
        assert!(t
            .commit(&LinkState::disconnected(), t0 + Duration::seconds(16))
            .is_none());
        let re = t
            .commit(&assoc("00:0b:86:11:22:02"), t0 + Duration::seconds(20))
            .unwrap();
        assert_eq!(re.kind, TransitionKind::Reconnect);
        t.commit(&LinkState::off(), t0 + Duration::seconds(25));
        let join = t
            .commit(&assoc("00:0b:86:11:22:03"), t0 + Duration::seconds(30))
            .unwrap();
        assert_eq!(join.kind, TransitionKind::Join);
        assert_eq!(t.generation, 6);
    }

    #[test]
    fn same_bssid_keeps_since_and_ap_name() {
        let mut t = AssociationTracker::default();
        let t0 = Local::now();
        let mut named = assoc("00:0b:86:11:22:01");
        named.link.ap_name = Some("ap-1".into());
        t.commit(&named, t0);
        t.commit(&assoc("00:0b:86:11:22:01"), t0 + Duration::seconds(3));
        let cur = t.current().unwrap();
        assert_eq!(cur.since, t0);
        assert_eq!(cur.ap_name.as_deref(), Some("ap-1"));
    }

    /// Location toggled off and on while running: the BSSID disappears and
    /// comes back. Neither change is a roam, and the association keeps the
    /// last known identity while it is withheld.
    #[test]
    fn withheld_bssid_is_not_a_roam() {
        let withheld = LinkState::associated(LinkSample {
            channel: Some(36),
            rssi_dbm: Some(-60),
            ..Default::default()
        });
        let mut t = AssociationTracker::default();
        let t0 = Local::now();
        let mut named = assoc("00:0b:86:11:22:01");
        named.link.ap_name = Some("ap-1".into());
        t.commit(&named, t0);
        assert!(t.commit(&withheld, t0 + Duration::seconds(5)).is_none());
        let cur = t.current().unwrap();
        assert_eq!(cur.bssid.as_deref(), Some("00:0b:86:11:22:01"));
        assert_eq!(cur.ssid.as_deref(), Some("Corp"));
        assert_eq!(cur.ap_name.as_deref(), Some("ap-1"));
        assert_eq!(cur.since, t0);
        assert!(t
            .commit(&assoc("00:0b:86:11:22:01"), t0 + Duration::seconds(10))
            .is_none());
        // Location came back on a different AP: that is a real roam.
        assert!(t.commit(&withheld, t0 + Duration::seconds(15)).is_none());
        let roam = t
            .commit(&assoc("00:0b:86:11:22:02"), t0 + Duration::seconds(20))
            .unwrap();
        assert_eq!(roam.kind, TransitionKind::Roam);
        assert_eq!(
            roam.from.as_ref().unwrap().bssid.as_deref(),
            Some("00:0b:86:11:22:01")
        );
    }

    /// Started with Location off: JOIN with no BSSID, and a later join with
    /// no BSSID is still a JOIN rather than a RECONNECT.
    #[test]
    fn withheld_bssid_joins_are_not_reconnects() {
        let withheld = LinkState::associated(LinkSample {
            channel: Some(36),
            rssi_dbm: Some(-60),
            ..Default::default()
        });
        let mut t = AssociationTracker::default();
        let t0 = Local::now();
        assert_eq!(t.commit(&withheld, t0).unwrap().kind, TransitionKind::Join);
        t.commit(&LinkState::disconnected(), t0 + Duration::seconds(5));
        assert_eq!(
            t.commit(&withheld, t0 + Duration::seconds(10))
                .unwrap()
                .kind,
            TransitionKind::Join
        );
    }
}
