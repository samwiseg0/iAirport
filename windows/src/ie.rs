//! 802.11 information element parsing: AP names, RSN, operation elements.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InformationElement {
    pub id: u8,
    pub body: Vec<u8>,
}

/// Splits an IE blob into elements. Stops at the first truncated element.
pub fn parse(data: &[u8]) -> Vec<InformationElement> {
    let mut elements = Vec::new();
    let mut index = 0;
    while index + 1 < data.len() {
        let id = data[index];
        let length = data[index + 1] as usize;
        let start = index + 2;
        let end = start + length;
        if end > data.len() {
            break;
        }
        elements.push(InformationElement {
            id,
            body: data[start..end].to_vec(),
        });
        index = end;
    }
    elements
}

/// AP name layouts follow the Wireshark dissector (packet-ieee80211.c, read
/// 2026-10-05). Aruba was also checked against live beacons. Offsets are into
/// the vendor IE body after the 3-byte OUI.
pub mod ap_name {
    use super::{parse, InformationElement};

    pub const MAX_LENGTH: usize = 64;

    enum Layout {
        Typed(u8),
        TypedSubtyped(u8, u8),
        Aerohive,
        Extreme,
        Fortinet,
        Arista,
    }

    struct Vendor {
        oui: [u8; 3],
        layout: Layout,
    }

    const VENDORS: [Vendor; 14] = [
        Vendor {
            oui: [0x00, 0x0b, 0x86],
            layout: Layout::TypedSubtyped(0x01, 0x03),
        },
        Vendor {
            oui: [0x00, 0x40, 0x96],
            layout: Layout::Typed(47),
        },
        Vendor {
            oui: [0x00, 0xe0, 0xfc],
            layout: Layout::TypedSubtyped(0x01, 0x01),
        },
        Vendor {
            oui: [0x00, 0x19, 0x77],
            layout: Layout::Aerohive,
        },
        Vendor {
            oui: [0x00, 0xa0, 0xf8],
            layout: Layout::Extreme,
        },
        Vendor {
            oui: [0x00, 0x09, 0x0f],
            layout: Layout::Fortinet,
        },
        Vendor {
            oui: [0x00, 0x11, 0x74],
            layout: Layout::Arista,
        },
        Vendor {
            oui: [0x5c, 0x5b, 0x35],
            layout: Layout::Typed(1),
        },
        Vendor {
            oui: [0x00, 0x15, 0x6d],
            layout: Layout::Typed(1),
        },
        Vendor {
            oui: [0x00, 0x13, 0x92],
            layout: Layout::Typed(3),
        },
        Vendor {
            oui: [0xdc, 0x08, 0x56],
            layout: Layout::Typed(1),
        },
        Vendor {
            oui: [0x48, 0xd0, 0x17],
            layout: Layout::Typed(2),
        },
        Vendor {
            oui: [0x3c, 0xb9, 0xa6],
            layout: Layout::Typed(1),
        },
        Vendor {
            oui: [0x84, 0x80, 0x94],
            layout: Layout::Typed(0),
        },
    ];

    pub fn from_blob(data: &[u8]) -> Option<String> {
        from_elements(&parse(data))
    }

    pub fn from_elements(elements: &[InformationElement]) -> Option<String> {
        elements.iter().find_map(decode)
    }

    fn decode(element: &InformationElement) -> Option<String> {
        let body = &element.body;
        match element.id {
            0xdd => {
                if body.len() < 4 {
                    return None;
                }
                let vendor = VENDORS.iter().find(|v| v.oui == body[..3])?;
                decode_layout(&vendor.layout, &body[3..])
            }
            // Cisco CCX1 CKIP + Device Name: 10 unknown bytes, 16-byte name, client count.
            0x85 => {
                if body.len() < 26 {
                    return None;
                }
                clean(&body[10..26])
            }
            _ => None,
        }
    }

    fn decode_layout(layout: &Layout, p: &[u8]) -> Option<String> {
        match layout {
            Layout::Typed(t) => {
                if p.len() > 1 && p[0] == *t {
                    clean(&p[1..])
                } else {
                    None
                }
            }
            Layout::TypedSubtyped(t, s) => {
                if p.len() > 3 && p[0] == *t && p[1] == *s {
                    clean(&p[3..])
                } else {
                    None
                }
            }
            Layout::Aerohive => {
                if p.len() > 4 && p[0] == 33 {
                    clean(prefixed(p, 3))
                } else {
                    None
                }
            }
            Layout::Extreme => {
                if p.len() > 9 && p[0] == 1 {
                    clean(prefixed(p, 8))
                } else {
                    None
                }
            }
            Layout::Fortinet => {
                if !(p.len() > 4 && p[0] == 10 && p[1] == 0) {
                    return None;
                }
                let mut index = 2;
                while index + 2 <= p.len() {
                    let kind = p[index];
                    let length = p[index + 1] as usize;
                    let start = index + 2;
                    let end = (start + length).min(p.len());
                    if kind == 1 {
                        return clean(&p[start..end]);
                    }
                    index = end;
                }
                None
            }
            Layout::Arista => {
                if p.len() > 1 && p[0] == 6 {
                    return clean(&p[1..]);
                }
                if p.len() > 3 && p[0] == 0 && p[1] == 6 {
                    clean(&p[3..])
                } else {
                    None
                }
            }
        }
    }

    fn prefixed(p: &[u8], length_at: usize) -> &[u8] {
        let start = length_at + 1;
        let end = (start + p[length_at] as usize).min(p.len());
        &p[start..end]
    }

    /// Trims NUL and space padding and rejects names with control characters.
    pub fn clean(bytes: &[u8]) -> Option<String> {
        let mut end = bytes.len();
        while end > 0 && (bytes[end - 1] == 0 || bytes[end - 1] == 0x20) {
            end -= 1;
        }
        let trimmed = &bytes[..end];
        if trimmed.is_empty() || trimmed.len() > MAX_LENGTH {
            return None;
        }
        let name = std::str::from_utf8(trimmed).ok()?;
        if name.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f) {
            return None;
        }
        Some(name.to_string())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SecurityInfo {
    pub security: Option<String>,
    pub ft: bool,
    pub mfp_capable: bool,
    pub mfp_required: bool,
}

impl SecurityInfo {
    /// The status line and JSON `mfp` field are true when the AP negotiates
    /// PMF at all, capable or required.
    pub fn mfp(&self) -> bool {
        self.mfp_capable || self.mfp_required
    }
}

/// Reads the RSN element (48) and the WPA vendor element (dd 00:50:f2:01)
/// and derives the CoreWLAN-style security string, FT and MFP flags.
pub fn security(elements: &[InformationElement]) -> SecurityInfo {
    let rsn = elements.iter().find(|e| e.id == 48);
    let wpa = elements
        .iter()
        .find(|e| e.id == 0xdd && e.body.len() >= 4 && e.body[..4] == [0x00, 0x50, 0xf2, 0x01]);
    let mut info = SecurityInfo::default();
    let Some(rsn) = rsn else {
        if let Some(wpa) = wpa {
            let akms = suite_list(&wpa.body[4..], [0x00, 0x50, 0xf2]);
            info.security = Some(if akms.contains(&1) {
                "wpa-enterprise".to_string()
            } else {
                "wpa-personal".to_string()
            });
        } else {
            info.security = Some("open".to_string());
        }
        return info;
    };
    let akms = suite_list(&rsn.body, [0x00, 0x0f, 0xac]);
    let capabilities = rsn_capabilities(&rsn.body);
    info.mfp_capable = capabilities & 0x0080 != 0;
    info.mfp_required = capabilities & 0x0040 != 0;
    info.ft = akms.iter().any(|a| matches!(a, 3 | 4 | 9 | 13));
    let sae = akms.iter().any(|a| matches!(a, 8 | 9 | 24 | 25));
    let psk = akms.iter().any(|a| matches!(a, 2 | 4 | 6));
    let suite_b = akms.iter().any(|a| matches!(a, 12 | 13));
    let eap = akms.iter().any(|a| matches!(a, 1 | 3 | 5));
    let owe = akms.contains(&18);
    let name = if owe {
        "owe"
    } else if sae && psk {
        "wpa3-transition"
    } else if sae {
        "wpa3-personal"
    } else if suite_b {
        "wpa3-enterprise"
    } else if eap && wpa.is_some() {
        "wpa-mixed-enterprise"
    } else if eap {
        "wpa2-enterprise"
    } else if psk && wpa.is_some() {
        "wpa-mixed"
    } else {
        "wpa2-personal"
    };
    info.security = Some(name.to_string());
    info
}

/// AKM suite selectors from an RSN or WPA body. The layout is version(2),
/// group cipher(4), pairwise count(2) + suites, AKM count(2) + suites.
fn suite_list(body: &[u8], oui: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    if body.len() < 8 {
        return out;
    }
    let pairwise = u16::from_le_bytes([body[6], body[7]]) as usize;
    let akm_count_at = 8 + pairwise * 4;
    if body.len() < akm_count_at + 2 {
        return out;
    }
    let akm_count = u16::from_le_bytes([body[akm_count_at], body[akm_count_at + 1]]) as usize;
    let mut at = akm_count_at + 2;
    for _ in 0..akm_count {
        if body.len() < at + 4 {
            break;
        }
        if body[at..at + 3] == oui {
            out.push(body[at + 3]);
        }
        at += 4;
    }
    out
}

fn rsn_capabilities(body: &[u8]) -> u16 {
    if body.len() < 8 {
        return 0;
    }
    let pairwise = u16::from_le_bytes([body[6], body[7]]) as usize;
    let akm_count_at = 8 + pairwise * 4;
    if body.len() < akm_count_at + 2 {
        return 0;
    }
    let akm_count = u16::from_le_bytes([body[akm_count_at], body[akm_count_at + 1]]) as usize;
    let cap_at = akm_count_at + 2 + akm_count * 4;
    if body.len() < cap_at + 2 {
        return 0;
    }
    u16::from_le_bytes([body[cap_at], body[cap_at + 1]])
}

/// Channel width in MHz from HT (61), VHT (192), HE (255/36) and EHT (255/106)
/// operation elements. The widest advertised width wins.
pub fn channel_width(elements: &[InformationElement]) -> Option<u32> {
    let mut width: Option<u32> = None;
    if let Some(ht) = elements.iter().find(|e| e.id == 61 && e.body.len() >= 2) {
        width = Some(if ht.body[1] & 0x04 != 0 { 40 } else { 20 });
    }
    if let Some(vht) = elements.iter().find(|e| e.id == 192 && e.body.len() >= 3) {
        let ccfs0 = vht.body[1];
        let ccfs1 = vht.body[2];
        let vht_width = match vht.body[0] {
            1 => {
                if ccfs1 == 0 {
                    80
                } else {
                    let diff = (ccfs0 as i32 - ccfs1 as i32).abs();
                    if diff == 8 {
                        160
                    } else {
                        80
                    }
                }
            }
            2 => 160,
            3 => 80,
            _ => 0,
        };
        if vht_width > width.unwrap_or(0) {
            width = Some(vht_width);
        }
    }
    for element in elements
        .iter()
        .filter(|e| e.id == 255 && !e.body.is_empty())
    {
        match element.body[0] {
            36 => {
                if let Some(w) = he_width(&element.body[1..]) {
                    if w > width.unwrap_or(0) {
                        width = Some(w);
                    }
                }
            }
            106 => {
                if let Some(w) = eht_width(&element.body[1..]) {
                    if w > width.unwrap_or(0) {
                        width = Some(w);
                    }
                }
            }
            _ => {}
        }
    }
    width
}

fn he_width(body: &[u8]) -> Option<u32> {
    // HE Operation Parameters (3 bytes), BSS Color (1), Basic MCS (2), then
    // optional VHT Operation Info (3), Co-Hosted BSS (1), 6 GHz Operation Info (5).
    if body.len() < 6 {
        return None;
    }
    let params = u32::from_le_bytes([body[0], body[1], body[2], 0]);
    let vht_present = params & (1 << 14) != 0;
    let co_hosted = params & (1 << 15) != 0;
    let six_ghz = params & (1 << 17) != 0;
    if !six_ghz {
        return None;
    }
    let mut at = 6;
    if vht_present {
        at += 3;
    }
    if co_hosted {
        at += 1;
    }
    if body.len() < at + 2 {
        return None;
    }
    match body[at + 1] & 0x03 {
        0 => Some(20),
        1 => Some(40),
        2 => Some(80),
        _ => Some(160),
    }
}

fn eht_width(body: &[u8]) -> Option<u32> {
    // EHT Operation Parameters (1), Basic EHT MCS (4), then optional
    // Operation Information with the control byte first.
    if body.len() < 6 || body[0] & 0x01 == 0 {
        return None;
    }
    match body[5] & 0x07 {
        0 => Some(20),
        1 => Some(40),
        2 => Some(80),
        3 => Some(160),
        4 => Some(320),
        _ => None,
    }
}

pub fn channel_from_khz(khz: u32) -> Option<u32> {
    let mhz = khz / 1000;
    match mhz {
        2412..=2472 => Some((mhz - 2407) / 5),
        2484 => Some(14),
        5000..=5895 => Some((mhz - 5000) / 5),
        5935 => Some(2),
        5955..=7115 => Some((mhz - 5950) / 5),
        _ => None,
    }
}

pub fn band_from_khz(khz: u32) -> Option<&'static str> {
    let mhz = khz / 1000;
    match mhz {
        2400..=2500 => Some("2.4"),
        5000..=5900 => Some("5"),
        5925..=7125 => Some("6"),
        _ => None,
    }
}

/// Band from a bare channel number, used when no frequency is known. 6 GHz
/// channel numbers overlap 5 GHz, so a channel above 14 reads as 5 GHz here.
pub fn band_from_channel(channel: u32) -> Option<&'static str> {
    match channel {
        1..=14 => Some("2.4"),
        15..=200 => Some("5"),
        _ => None,
    }
}

/// DOT11_PHY_TYPE values to the names the macOS tool prints.
pub fn phy_name(dot11_phy_type: i32) -> Option<&'static str> {
    match dot11_phy_type {
        2 | 5 => Some("11b"),
        4 => Some("11a"),
        6 => Some("11g"),
        7 => Some("11n"),
        8 => Some("11ac"),
        10 => Some("11ax"),
        11 => Some("11be"),
        _ => None,
    }
}

/// Security name from DOT11_AUTH_ALGORITHM and DOT11_CIPHER_ALGORITHM, used
/// when no beacon IEs are available.
pub fn security_from_auth(auth: i32, cipher: i32) -> Option<&'static str> {
    match auth {
        1 => {
            if cipher == 0 {
                Some("open")
            } else {
                Some("wep")
            }
        }
        2 => Some("wep"),
        3 => Some("wpa-enterprise"),
        4 | 5 => Some("wpa-personal"),
        6 => Some("wpa2-enterprise"),
        7 => Some("wpa2-personal"),
        8 | 11 => Some("wpa3-enterprise"),
        9 => Some("wpa3-personal"),
        10 => Some("owe"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vendor_ie(oui: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut body = oui.to_vec();
        body.extend_from_slice(payload);
        let mut out = vec![0xdd, body.len() as u8];
        out.extend(body);
        out
    }

    fn name(oui: &[u8], payload: &[u8]) -> Option<String> {
        ap_name::from_blob(&vendor_ie(oui, payload))
    }

    #[test]
    fn aruba_ap_name() {
        let apname = b"AP-Lobby-01";
        let mut ssid = vec![0x00, 0x03];
        ssid.extend_from_slice(b"Net");
        let mut payload = vec![0x01, 0x03, 0x00];
        payload.extend_from_slice(apname);
        let aruba = vendor_ie(&[0x00, 0x0b, 0x86], &payload);
        let arm = vendor_ie(&[0x00, 0x0b, 0x86], &[0x01, 0x04, 0x08, 0x1b]);
        let mut blob = ssid;
        blob.extend(arm);
        blob.extend(aruba);
        assert_eq!(ap_name::from_blob(&blob).as_deref(), Some("AP-Lobby-01"));
        assert_eq!(parse(&blob).len(), 3);
    }

    #[test]
    fn cisco_ap_name_v2() {
        let mut payload = vec![47];
        payload.extend_from_slice(b"AP-Floor2-West");
        assert_eq!(
            name(&[0x00, 0x40, 0x96], &payload).as_deref(),
            Some("AP-Floor2-West")
        );
        assert_eq!(name(&[0x00, 0x40, 0x96], &[0x03, 0x05]), None);
    }

    #[test]
    fn cisco_ccx1_device_name() {
        let mut body = vec![0u8; 10];
        body.extend_from_slice(b"ap-ccx-01");
        body.extend(vec![0u8; 7]);
        body.extend_from_slice(&[0x02, 0x00, 0x00]);
        assert_eq!(body.len(), 29);
        let mut blob = vec![0x85, body.len() as u8];
        blob.extend(body);
        assert_eq!(ap_name::from_blob(&blob).as_deref(), Some("ap-ccx-01"));
        let mut short = vec![0x85, 20];
        short.extend(vec![0x41u8; 20]);
        assert_eq!(ap_name::from_blob(&short), None);
    }

    #[test]
    fn rejects_truncated_and_unprintable_names() {
        let truncated = [0xdd, 0x20, 0x00, 0x0b, 0x86, 0x01, 0x03, 0x00, 0x41];
        assert_eq!(ap_name::from_blob(&truncated), None);
        assert!(parse(&truncated).is_empty());
        assert_eq!(
            name(&[0x00, 0x0b, 0x86], &[0x01, 0x03, 0x00, 0x41, 0x07, 0x42]),
            None
        );
        let mut padded = vec![0x01, 0x03, 0x00];
        padded.extend_from_slice(b"AP 7  ");
        padded.extend_from_slice(&[0, 0]);
        assert_eq!(name(&[0x00, 0x0b, 0x86], &padded).as_deref(), Some("AP 7"));
        assert_eq!(ap_name::from_blob(&[]), None);
    }

    #[test]
    fn other_vendor_layouts() {
        let ap = b"AP-Test-01";
        let cat = |prefix: &[u8], tail: &[u8]| {
            let mut v = prefix.to_vec();
            v.extend_from_slice(ap);
            v.extend_from_slice(tail);
            v
        };
        assert_eq!(
            name(&[0x00, 0xe0, 0xfc], &cat(&[0x01, 0x01, 0x00], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(
                &[0x00, 0x19, 0x77],
                &cat(&[33, 0x01, 0x00, ap.len() as u8], &[0xff, 0xff])
            )
            .as_deref(),
            Some("AP-Test-01")
        );
        // lswifi example: 00 a0 f8 01 03 01 0f c0 00 00 00 06 'ap8533'
        let mut extreme = vec![0x01, 0x03, 0x01, 0x0f, 0xc0, 0x00, 0x00, 0x00, 0x06];
        extreme.extend_from_slice(b"ap8533");
        assert_eq!(
            name(&[0x00, 0xa0, 0xf8], &extreme).as_deref(),
            Some("ap8533")
        );
        let model = b"FAP231F";
        let mut fortinet = vec![10, 0, 2, model.len() as u8];
        fortinet.extend_from_slice(model);
        fortinet.push(1);
        fortinet.push(ap.len() as u8);
        fortinet.extend_from_slice(ap);
        assert_eq!(
            name(&[0x00, 0x09, 0x0f], &fortinet).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x00, 0x11, 0x74], &cat(&[6], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x00, 0x11, 0x74], &cat(&[0, 6, 0], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x5c, 0x5b, 0x35], &cat(&[1], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x00, 0x15, 0x6d], &cat(&[1], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x00, 0x13, 0x92], &cat(&[3], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0xdc, 0x08, 0x56], &cat(&[1], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x48, 0xd0, 0x17], &cat(&[2], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x3c, 0xb9, 0xa6], &cat(&[1], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(
            name(&[0x84, 0x80, 0x94], &cat(&[0], &[])).as_deref(),
            Some("AP-Test-01")
        );
        assert_eq!(name(&[0x00, 0x13, 0x92], &cat(&[1], &[])), None);
        assert_eq!(name(&[0x00, 0x50, 0xf2], &cat(&[2, 1, 1, 0x80], &[])), None);
    }

    fn rsn(akms: &[u8], caps: u16) -> Vec<u8> {
        let mut body = vec![
            0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04,
        ];
        body.extend_from_slice(&(akms.len() as u16).to_le_bytes());
        for akm in akms {
            body.extend_from_slice(&[0x00, 0x0f, 0xac, *akm]);
        }
        body.extend_from_slice(&caps.to_le_bytes());
        let mut out = vec![48, body.len() as u8];
        out.extend(body);
        out
    }

    #[test]
    fn rsn_security_ft_and_mfp() {
        let psk = security(&parse(&rsn(&[2], 0)));
        assert_eq!(psk.security.as_deref(), Some("wpa2-personal"));
        assert!(!psk.ft && !psk.mfp());

        let ft_eap = security(&parse(&rsn(&[1, 3], 0x0080)));
        assert_eq!(ft_eap.security.as_deref(), Some("wpa2-enterprise"));
        assert!(ft_eap.ft);
        assert!(ft_eap.mfp_capable && !ft_eap.mfp_required);

        let transition = security(&parse(&rsn(&[2, 8], 0x00c0)));
        assert_eq!(transition.security.as_deref(), Some("wpa3-transition"));
        assert!(transition.mfp_required);

        let sae_ft = security(&parse(&rsn(&[8, 9], 0x00c0)));
        assert_eq!(sae_ft.security.as_deref(), Some("wpa3-personal"));
        assert!(sae_ft.ft);

        let suite_b = security(&parse(&rsn(&[12, 13], 0x00c0)));
        assert_eq!(suite_b.security.as_deref(), Some("wpa3-enterprise"));
        assert!(suite_b.ft);

        assert_eq!(
            security(&parse(&rsn(&[18], 0x00c0))).security.as_deref(),
            Some("owe")
        );
        assert_eq!(security(&[]).security.as_deref(), Some("open"));

        let wpa = vendor_ie(
            &[0x00, 0x50, 0xf2],
            &[
                0x01, 0x01, 0x00, 0x00, 0x50, 0xf2, 0x02, 0x01, 0x00, 0x00, 0x50, 0xf2, 0x02, 0x01,
                0x00, 0x00, 0x50, 0xf2, 0x02,
            ],
        );
        assert_eq!(
            security(&parse(&wpa)).security.as_deref(),
            Some("wpa-personal")
        );
        let mut mixed = wpa.clone();
        mixed.extend(rsn(&[2], 0));
        assert_eq!(
            security(&parse(&mixed)).security.as_deref(),
            Some("wpa-mixed")
        );
    }

    #[test]
    fn width_from_operation_elements() {
        let ht20 = [
            61, 22, 36, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(channel_width(&parse(&ht20)), Some(20));
        let ht40 = [
            61, 22, 36, 0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(channel_width(&parse(&ht40)), Some(40));
        let mut vht80 = ht40.to_vec();
        vht80.extend_from_slice(&[192, 5, 1, 42, 0, 0xff, 0xff]);
        assert_eq!(channel_width(&parse(&vht80)), Some(80));
        let mut vht160 = ht40.to_vec();
        vht160.extend_from_slice(&[192, 5, 1, 42, 50, 0xff, 0xff]);
        assert_eq!(channel_width(&parse(&vht160)), Some(160));
        // HE operation with 6 GHz info present (bit 17), width 2 = 80 MHz.
        let he = [
            255, 12, 36, 0x00, 0x00, 0x02, 0x01, 0xff, 0xff, 37, 0x02, 39, 0, 0,
        ];
        assert_eq!(channel_width(&parse(&he)), Some(80));
        let eht = [255, 9, 106, 0x01, 0, 0, 0, 0, 0x04, 31, 0];
        assert_eq!(channel_width(&parse(&eht)), Some(320));
        assert_eq!(channel_width(&[]), None);
    }

    #[test]
    fn channel_and_band_from_frequency() {
        assert_eq!(channel_from_khz(2412000), Some(1));
        assert_eq!(channel_from_khz(2462000), Some(11));
        assert_eq!(channel_from_khz(2484000), Some(14));
        assert_eq!(channel_from_khz(5220000), Some(44));
        assert_eq!(channel_from_khz(5745000), Some(149));
        assert_eq!(channel_from_khz(5955000), Some(1));
        assert_eq!(channel_from_khz(6135000), Some(37));
        assert_eq!(channel_from_khz(5935000), Some(2));
        assert_eq!(channel_from_khz(1000), None);
        assert_eq!(band_from_khz(2437000), Some("2.4"));
        assert_eq!(band_from_khz(5220000), Some("5"));
        assert_eq!(band_from_khz(6135000), Some("6"));
        assert_eq!(band_from_channel(6), Some("2.4"));
        assert_eq!(band_from_channel(149), Some("5"));
    }

    #[test]
    fn phy_and_auth_names() {
        assert_eq!(phy_name(7), Some("11n"));
        assert_eq!(phy_name(8), Some("11ac"));
        assert_eq!(phy_name(10), Some("11ax"));
        assert_eq!(phy_name(11), Some("11be"));
        assert_eq!(phy_name(0), None);
        assert_eq!(security_from_auth(1, 0), Some("open"));
        assert_eq!(security_from_auth(1, 0x101), Some("wep"));
        assert_eq!(security_from_auth(7, 4), Some("wpa2-personal"));
        assert_eq!(security_from_auth(6, 4), Some("wpa2-enterprise"));
        assert_eq!(security_from_auth(9, 4), Some("wpa3-personal"));
        assert_eq!(security_from_auth(10, 4), Some("owe"));
    }
}
