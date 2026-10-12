use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::format::mac;

#[derive(Default)]
pub struct Oui {
    vendors: HashMap<String, String>,
    pub path: Option<PathBuf>,
}

impl Oui {
    pub fn from_map(vendors: HashMap<String, String>) -> Self {
        Oui {
            vendors,
            path: None,
        }
    }

    /// Searches `candidates` in order and loads the first file that exists.
    pub fn find(candidates: &[PathBuf]) -> (Oui, Option<String>) {
        for path in candidates {
            if path.is_file() {
                return (Oui::load(path), None);
            }
        }
        (
            Oui::default(),
            Some("OUI data file missing. Vendor names will be empty.".to_string()),
        )
    }

    pub fn load(path: &Path) -> Oui {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Oui {
                vendors: HashMap::new(),
                path: Some(path.to_path_buf()),
            };
        };
        let mut oui = Oui::parse(&text);
        oui.path = Some(path.to_path_buf());
        oui
    }

    /// One `XX:XX:XX<tab>Short<tab>Long` line per prefix. `#` starts a comment.
    pub fn parse(text: &str) -> Oui {
        let mut values = HashMap::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split([' ', '\t']).filter(|p| !p.is_empty());
            let (Some(key), Some(vendor)) = (parts.next(), parts.next()) else {
                continue;
            };
            let key = key.to_ascii_uppercase();
            if !is_oui_key(&key) {
                continue;
            }
            values.insert(key, vendor.to_string());
        }
        Oui::from_map(values)
    }

    pub fn len(&self) -> usize {
        self.vendors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vendors.is_empty()
    }

    pub fn vendor(&self, bssid: Option<&str>) -> Option<String> {
        let key = mac::oui(bssid)?;
        self.vendors.get(&key).cloned()
    }
}

fn is_oui_key(key: &str) -> bool {
    let b = key.as_bytes();
    b.len() == 8
        && b[2] == b':'
        && b[5] == b':'
        && [0, 1, 3, 4, 6, 7].iter().all(|&i| b[i].is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_from_map() {
        let mut map = HashMap::new();
        map.insert("00:0B:86".to_string(), "Aruba".to_string());
        let oui = Oui::from_map(map);
        assert_eq!(
            oui.vendor(Some("00:0b:86:11:22:01")).as_deref(),
            Some("Aruba")
        );
        assert_eq!(oui.vendor(Some("?")), None);
    }

    #[test]
    fn parse_repo_format() {
        let text = "# Source: https://standards-oui.ieee.org/oui/oui.csv\n# Updated: 2026-10-05\n28:6F:B9\tNokia\tNokia Shanghai Bell Co., Ltd.\n00:0b:86\tAruba\tAruba, a Hewlett Packard Enterprise Company\nbad line\nZZ:00:00\tNope\n";
        let oui = Oui::parse(text);
        assert_eq!(oui.len(), 2);
        assert_eq!(
            oui.vendor(Some("28:6f:b9:00:00:01")).as_deref(),
            Some("Nokia")
        );
        assert_eq!(
            oui.vendor(Some("00:0B:86:11:22:01")).as_deref(),
            Some("Aruba")
        );
    }

    #[test]
    fn find_reports_missing_file() {
        let (oui, warning) = Oui::find(&[PathBuf::from("does-not-exist/oui.txt")]);
        assert!(oui.is_empty());
        assert_eq!(
            warning.as_deref(),
            Some("OUI data file missing. Vendor names will be empty.")
        );
    }
}
