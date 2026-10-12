use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ipv6Kind {
    LinkLocal,
    Slaac,
    Temporary,
    Dhcpv6,
    Static,
    Deprecated,
}

impl Ipv6Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Ipv6Kind::LinkLocal => "linkLocal",
            Ipv6Kind::Slaac => "slaac",
            Ipv6Kind::Temporary => "temporary",
            Ipv6Kind::Dhcpv6 => "dhcpv6",
            Ipv6Kind::Static => "static",
            Ipv6Kind::Deprecated => "deprecated",
        }
    }
}

/// Address origin as IP Helper reports it. The numbers are NL_PREFIX_ORIGIN,
/// NL_SUFFIX_ORIGIN and NL_DAD_STATE values. `expired` is PreferredLifetime
/// == 0. The lifetime itself stays out, it counts down between samples and
/// would make every IpState compare unequal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ipv6Origin {
    pub prefix_origin: u32,
    pub suffix_origin: u32,
    pub dad_state: u32,
    pub expired: bool,
}

pub const PREFIX_ORIGIN_DHCP: u32 = 3;
pub const PREFIX_ORIGIN_ROUTER_ADVERTISEMENT: u32 = 4;
pub const SUFFIX_ORIGIN_RANDOM: u32 = 5;
pub const DAD_STATE_DEPRECATED: u32 = 3;
pub const DAD_STATE_PREFERRED: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ipv6AddressInfo {
    pub address: String,
    pub prefix: Option<u32>,
    pub origin: Ipv6Origin,
}

impl Ipv6AddressInfo {
    pub fn new(address: &str, prefix: Option<u32>, origin: Ipv6Origin) -> Self {
        Ipv6AddressInfo {
            address: address.to_string(),
            prefix,
            origin,
        }
    }

    /// Same order as the macOS flag test: link-local, deprecated, temporary,
    /// DHCPv6, SLAAC, then static.
    pub fn kind(&self) -> Ipv6Kind {
        let lower = self.address.to_ascii_lowercase();
        if lower.starts_with("fe80:") || lower.starts_with("fe80%") {
            return Ipv6Kind::LinkLocal;
        }
        let o = &self.origin;
        if o.expired || o.dad_state == DAD_STATE_DEPRECATED {
            return Ipv6Kind::Deprecated;
        }
        if o.suffix_origin == SUFFIX_ORIGIN_RANDOM {
            return Ipv6Kind::Temporary;
        }
        if o.prefix_origin == PREFIX_ORIGIN_DHCP {
            return Ipv6Kind::Dhcpv6;
        }
        if o.prefix_origin == PREFIX_ORIGIN_ROUTER_ADVERTISEMENT {
            return Ipv6Kind::Slaac;
        }
        Ipv6Kind::Static
    }

    pub fn is_global(&self) -> bool {
        self.kind() != Ipv6Kind::LinkLocal
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IpState {
    pub ipv4: Vec<String>,
    pub ipv4_prefixes: Vec<Option<u32>>,
    pub ipv4_router: Option<String>,
    pub ipv6: Vec<Ipv6AddressInfo>,
    pub ipv6_router: Option<String>,
    pub primary_v4: bool,
    pub primary_v6: bool,
}

impl IpState {
    /// APIPA (169.254/16) means DHCP failed, so it does not count as ready.
    pub fn has_ipv4(&self) -> bool {
        self.ipv4.iter().any(|a| !a.starts_with("169.254."))
    }

    pub fn has_global_ipv6(&self) -> bool {
        self.ipv6
            .iter()
            .any(|a| a.is_global() && a.kind() != Ipv6Kind::Deprecated)
    }

    pub fn display_ipv6(&self) -> Option<&Ipv6AddressInfo> {
        let globals: Vec<&Ipv6AddressInfo> = self.ipv6.iter().filter(|a| a.is_global()).collect();
        if let Some(stable) = globals
            .iter()
            .find(|a| matches!(a.kind(), Ipv6Kind::Slaac | Ipv6Kind::Dhcpv6))
        {
            return Some(stable);
        }
        if let Some(live) = globals.iter().find(|a| a.kind() != Ipv6Kind::Deprecated) {
            return Some(live);
        }
        globals.first().copied()
    }

    pub fn temporary_ipv6_count_for_display(&self) -> usize {
        let shown = self.display_ipv6().map(|a| a.address.clone());
        self.ipv6
            .iter()
            .filter(|a| {
                a.is_global()
                    && a.kind() == Ipv6Kind::Temporary
                    && Some(&a.address) != shown.as_ref()
            })
            .count()
    }

    pub fn display_line(&self) -> String {
        let v4 = match self.ipv4.first() {
            Some(addr) => {
                let prefix = self
                    .ipv4_prefixes
                    .first()
                    .copied()
                    .flatten()
                    .map(|p| format!("/{p}"))
                    .unwrap_or_default();
                format!(
                    "v4 {}{} gw {}",
                    addr,
                    prefix,
                    self.ipv4_router.as_deref().unwrap_or("none")
                )
            }
            None => "v4 none".to_string(),
        };
        let v6 = match self.display_ipv6() {
            Some(shown) => {
                let prefix = shown.prefix.map(|p| format!("/{p}")).unwrap_or_default();
                let temp_count = self.temporary_ipv6_count_for_display();
                let temp = if temp_count > 0 {
                    format!(" (+{temp_count} temp)")
                } else {
                    String::new()
                };
                format!(
                    "v6 {}{} {}{} gw {}",
                    shown.address,
                    prefix,
                    shown.kind().as_str(),
                    temp,
                    self.ipv6_router.as_deref().unwrap_or("none")
                )
            }
            None => "v6 none".to_string(),
        };
        format!("IP  {v4}  |  {v6}")
    }

    pub fn compact_tag(&self) -> String {
        let v4 = if self.has_ipv4() { "v4" } else { "-" };
        let v6 = if self.has_global_ipv6() { "v6" } else { "-" };
        format!("{v4} {v6}")
    }

    /// Same shape as the macOS `ip` object. Windows has no flag word, so each
    /// v6 entry carries `prefix_origin` and `suffix_origin` in place of `flags`.
    pub fn json_object(&self) -> Value {
        let v6: Vec<Value> = self
            .ipv6
            .iter()
            .map(|a| {
                let mut entry = json!({
                    "addr": a.address,
                    "kind": a.kind().as_str(),
                    "prefix_origin": a.origin.prefix_origin,
                    "suffix_origin": a.origin.suffix_origin,
                });
                if let Some(prefix) = a.prefix {
                    entry["prefix"] = json!(prefix);
                }
                entry
            })
            .collect();
        let mut object = json!({
            "v4": self.ipv4,
            "v4_prefix": self.ipv4_prefixes,
            "v6": v6,
            "primary_v4": self.primary_v4,
            "primary_v6": self.primary_v6,
        });
        if let Some(gw) = &self.ipv4_router {
            object["v4_gw"] = json!(gw);
        }
        if let Some(gw) = &self.ipv6_router {
            object["v6_gw"] = json!(gw);
        }
        object
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpAfterRoam {
    Kept,
    Changed(i64),
    Renewed(i64),
}

impl IpAfterRoam {
    pub fn csv_value(&self) -> &'static str {
        match self {
            IpAfterRoam::Kept => "kept",
            IpAfterRoam::Changed(_) => "changed",
            IpAfterRoam::Renewed(_) => "renewed",
        }
    }

    pub fn milliseconds(&self) -> Option<i64> {
        match self {
            IpAfterRoam::Kept => None,
            IpAfterRoam::Changed(ms) | IpAfterRoam::Renewed(ms) => Some(*ms),
        }
    }

    pub fn display(&self, family: &str) -> String {
        match self {
            IpAfterRoam::Kept => format!("{family} kept"),
            IpAfterRoam::Changed(ms) => format!("{} changed {:.1} s", family, *ms as f64 / 1000.0),
            IpAfterRoam::Renewed(ms) => format!("{} renewed {:.1} s", family, *ms as f64 / 1000.0),
        }
    }
}

pub fn compare_v4(before: &IpState, after: &IpState, milliseconds: i64) -> IpAfterRoam {
    let b: BTreeSet<&String> = before.ipv4.iter().collect();
    let a: BTreeSet<&String> = after.ipv4.iter().collect();
    if a == b {
        IpAfterRoam::Kept
    } else {
        IpAfterRoam::Renewed(milliseconds)
    }
}

pub fn compare_v6(before: &IpState, after: &IpState, milliseconds: i64) -> IpAfterRoam {
    let b: BTreeSet<&String> = before
        .ipv6
        .iter()
        .filter(|a| a.is_global())
        .map(|a| &a.address)
        .collect();
    let a: BTreeSet<&String> = after
        .ipv6
        .iter()
        .filter(|a| a.is_global())
        .map(|a| &a.address)
        .collect();
    if a == b {
        IpAfterRoam::Kept
    } else {
        IpAfterRoam::Changed(milliseconds)
    }
}

pub fn ipv4_prefix_from_mask(mask: &str) -> Option<u32> {
    let parts: Vec<u32> = mask.split('.').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 4 {
        return None;
    }
    Some(parts.iter().map(|p| p.count_ones()).sum())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slaac() -> Ipv6Origin {
        Ipv6Origin {
            prefix_origin: 4,
            suffix_origin: 4,
            dad_state: 4,
            expired: false,
        }
    }

    #[test]
    fn kinds_and_display() {
        let link_local = Ipv6AddressInfo::new(
            "fe80::1",
            Some(64),
            Ipv6Origin {
                prefix_origin: 2,
                suffix_origin: 4,
                dad_state: 4,
                expired: false,
            },
        );
        let stable = Ipv6AddressInfo::new("2600::1", Some(64), slaac());
        let temp = Ipv6AddressInfo::new(
            "2600::2",
            Some(64),
            Ipv6Origin {
                suffix_origin: 5,
                ..slaac()
            },
        );
        let deprecated = Ipv6AddressInfo::new(
            "2600::3",
            Some(64),
            Ipv6Origin {
                expired: true,
                ..slaac()
            },
        );
        let dad_deprecated = Ipv6AddressInfo::new(
            "2600::4",
            Some(64),
            Ipv6Origin {
                dad_state: 3,
                ..slaac()
            },
        );
        let dhcp = Ipv6AddressInfo::new(
            "2600::5",
            Some(128),
            Ipv6Origin {
                prefix_origin: 3,
                suffix_origin: 3,
                ..slaac()
            },
        );
        let manual = Ipv6AddressInfo::new(
            "2600::6",
            Some(64),
            Ipv6Origin {
                prefix_origin: 1,
                suffix_origin: 1,
                ..slaac()
            },
        );
        assert_eq!(link_local.kind(), Ipv6Kind::LinkLocal);
        assert_eq!(stable.kind(), Ipv6Kind::Slaac);
        assert_eq!(temp.kind(), Ipv6Kind::Temporary);
        assert_eq!(deprecated.kind(), Ipv6Kind::Deprecated);
        assert_eq!(dad_deprecated.kind(), Ipv6Kind::Deprecated);
        assert_eq!(dhcp.kind(), Ipv6Kind::Dhcpv6);
        assert_eq!(manual.kind(), Ipv6Kind::Static);
        let state = IpState {
            ipv6: vec![link_local, temp, stable],
            ..Default::default()
        };
        assert_eq!(
            state.display_ipv6().map(|a| a.address.as_str()),
            Some("2600::1")
        );
        assert_eq!(state.temporary_ipv6_count_for_display(), 1);
        assert_eq!(
            state.display_line(),
            "IP  v4 none  |  v6 2600::1/64 slaac (+1 temp) gw none"
        );
        assert_eq!(state.compact_tag(), "- v6");
    }

    #[test]
    fn display_line_matches_docs() {
        let state = IpState {
            ipv4: vec!["192.0.2.143".to_string()],
            ipv4_prefixes: vec![Some(24)],
            ipv4_router: Some("192.0.2.1".to_string()),
            ipv6: vec![
                Ipv6AddressInfo::new("2001:db8:1::1234", Some(64), slaac()),
                Ipv6AddressInfo::new(
                    "2001:db8:1::abcd",
                    Some(64),
                    Ipv6Origin {
                        suffix_origin: 5,
                        ..slaac()
                    },
                ),
            ],
            ipv6_router: Some("fe80::1".to_string()),
            primary_v4: true,
            primary_v6: true,
        };
        assert_eq!(
            state.display_line(),
            "IP  v4 192.0.2.143/24 gw 192.0.2.1  |  v6 2001:db8:1::1234/64 slaac (+1 temp) gw fe80::1"
        );
        let json = state.json_object();
        assert_eq!(json["v4"][0], "192.0.2.143");
        assert_eq!(json["v4_prefix"][0], 24);
        assert_eq!(json["v4_gw"], "192.0.2.1");
        assert_eq!(json["v6"][0]["kind"], "slaac");
        assert_eq!(json["v6"][0]["prefix"], 64);
        assert_eq!(json["v6_gw"], "fe80::1");
        assert_eq!(json["primary_v4"], true);
        let empty = IpState::default();
        assert_eq!(empty.display_line(), "IP  v4 none  |  v6 none");
    }

    #[test]
    fn after_roam_comparison_and_wording() {
        let before = IpState {
            ipv4: vec!["192.0.2.10".into()],
            ipv6: vec![Ipv6AddressInfo::new("2600::1", Some(64), slaac())],
            ..Default::default()
        };
        let same = before.clone();
        let changed = IpState {
            ipv4: vec!["192.0.2.11".into()],
            ipv6: vec![Ipv6AddressInfo::new("2600::2", Some(64), slaac())],
            ..Default::default()
        };
        assert_eq!(compare_v4(&before, &same, 100), IpAfterRoam::Kept);
        assert_eq!(compare_v6(&before, &same, 100), IpAfterRoam::Kept);
        assert_eq!(
            compare_v4(&before, &changed, 1300),
            IpAfterRoam::Renewed(1300)
        );
        assert_eq!(
            compare_v6(&before, &changed, 2100),
            IpAfterRoam::Changed(2100)
        );
        let line = format!(
            "IP after roam  {}  {}",
            compare_v4(&before, &same, 2100).display("v4"),
            compare_v6(&before, &changed, 2100).display("v6")
        );
        assert_eq!(line, "IP after roam  v4 kept  v6 changed 2.1 s");
        assert_eq!(IpAfterRoam::Renewed(1300).display("v4"), "v4 renewed 1.3 s");
        assert_eq!(IpAfterRoam::Renewed(1300).csv_value(), "renewed");
        assert_eq!(IpAfterRoam::Kept.milliseconds(), None);
    }

    #[test]
    fn mask_to_prefix() {
        assert_eq!(ipv4_prefix_from_mask("255.255.255.0"), Some(24));
        assert_eq!(ipv4_prefix_from_mask("255.255.0.0"), Some(16));
        assert_eq!(ipv4_prefix_from_mask("bad"), None);
    }
}
