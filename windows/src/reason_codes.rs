//! IEEE 802.11 reason codes. These are the codes in deauthentication and
//! disassociation frames. WLAN_REASON_CODE values from wlanapi are another
//! namespace and go through `WlanReasonCodeToString` instead.

pub fn text(code: u32) -> &'static str {
    match code {
        1 => "unspecified",
        2 => "previous authentication no longer valid",
        3 => "deauthenticated because station is leaving",
        4 => "disassociated due to inactivity",
        5 => "AP unable to handle stations",
        6 => "class 2 frame from nonauthenticated station",
        7 => "class 3 frame from nonassociated station",
        8 => "disassociated because station is leaving",
        9 => "station requesting association is not authenticated",
        10 => "power capability is not valid",
        11 => "supported channels are not valid",
        12 => "BSS transition management",
        13 => "invalid element",
        14 => "message integrity code failure",
        15 => "four-way handshake timeout",
        16 => "group key handshake timeout",
        17 => "element differs from request",
        18 => "invalid group cipher",
        19 => "invalid pairwise cipher",
        20 => "invalid AKMP",
        21 => "unsupported RSNE version",
        22 => "invalid RSNE capabilities",
        23 => "802.1X authentication failed",
        24 => "cipher suite rejected",
        25 => "TDLS direct link teardown",
        26 => "TDLS direct link teardown for unspecified reason",
        27 => "SSP request not accepted",
        28 => "no SSP roaming agreement",
        29 => "bad cipher or AKM",
        30 => "not authorized in this location",
        31 => "service change precludes TS",
        32 => "unspecified QoS reason",
        33 => "insufficient QoS bandwidth",
        34 => "too many frames need acknowledgment",
        35 => "station outside TXOP limits",
        36 => "station leaving QBSS",
        37 => "station does not want mechanism",
        38 => "station received frames using mechanism",
        39 => "requested service not authorized",
        40 => "requested service not supported",
        41 => "measurement request timeout",
        42 => "station leaving ESS",
        43 => "mesh peer canceled",
        44 => "maximum peers reached",
        45 => "mesh configuration policy violation",
        46 => "mesh close received",
        47 => "mesh maximum retries",
        48 => "mesh confirm timeout",
        49 => "mesh invalid GTK",
        50 => "mesh inconsistent parameters",
        51 => "mesh invalid security capability",
        52 => "mesh path error no proxy information",
        53 => "mesh path error no forwarding information",
        54 => "mesh path error destination unreachable",
        55 => "MAC address already exists in mesh",
        56 => "mesh channel switch regulatory requirement",
        57 => "mesh channel switch unspecified",
        58 => "poor channel conditions",
        59 => "BSS transition disassociation imminent",
        60 => "BSS transition excessive frame loss",
        61 => "BSS transition excessive delay",
        62 => "BSS transition candidate rejected",
        63 => "MCCAOP reservation conflict",
        64 => "MCCA tracking not supported",
        65 => "fast transition rejected by AP",
        66 => "authentication rejected due to anti-clogging token",
        _ => "unknown",
    }
}

pub fn suffix(code: u32) -> &'static str {
    if code == 3 {
        " (could be a ClientMatch move)"
    } else {
        ""
    }
}

pub fn deauth_line(kind: &str, reason: u32, source: Option<&str>) -> String {
    let from = source.map(|s| format!(" from {s}")).unwrap_or_default();
    format!(
        "{}  reason {} {}{}{}",
        kind,
        reason,
        text(reason),
        suffix(reason),
        from
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup() {
        assert_eq!(text(3), "deauthenticated because station is leaving");
        assert_eq!(text(999), "unknown");
        assert_eq!(
            deauth_line("DEAUTH", 3, None),
            "DEAUTH  reason 3 deauthenticated because station is leaving (could be a ClientMatch move)"
        );
        assert_eq!(
            deauth_line("DISASSOC", 8, Some("00:0b:86:11:22:01")),
            "DISASSOC  reason 8 disassociated because station is leaving from 00:0b:86:11:22:01"
        );
    }
}
