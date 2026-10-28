// SPDX-License-Identifier: MIT
//! Pinned Windows dynamic-time-zone key -> IANA mapping.
//!
//! This is a deliberately small, reviewable subset of CLDR `windowsZones`
//! (version 48): the 001 defaults plus territory overrides for common zones.
//! An unknown key or territory yields `None`, and the caller must show the
//! device row as unavailable rather than guess.

use std::collections::HashMap;
use std::sync::OnceLock;

pub const WINDOWS_ZONE_DATA_VERSION: &str = "CLDR 48 (pinned subset)";

const DATA: &str = include_str!("../data/windows-zones.tsv");

type Map = HashMap<(&'static str, &'static str), &'static str>;

fn table() -> &'static Map {
    static TABLE: OnceLock<Map> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut m = Map::new();
        for line in DATA.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            let (Some(id), Some(territory), Some(iana)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            // DATA is a &'static str, so its line slices are static too.
            m.insert((id, territory), iana);
        }
        m
    })
}

/// Map a Windows zone key (as returned by `GetDynamicTimeZoneInformation`) to an
/// IANA zone. `territory` is an ISO 3166-1 alpha-2 code, when available.
pub fn map_windows_zone(windows_id: &str, territory: Option<&str>) -> Option<&'static str> {
    let key = windows_id.trim();
    if key.is_empty() {
        return None;
    }
    let t = table();
    // Case-insensitive Windows key lookup, exact territory first, then 001.
    let wanted = key.to_ascii_lowercase();
    let terr = territory.map(|x| x.trim().to_ascii_uppercase());
    let mut default = None;
    for ((id, territory), iana) in t.iter() {
        if id.to_ascii_lowercase() != wanted {
            continue;
        }
        if *territory == "*" {
            default = Some(*iana);
        }
        if let Some(want) = &terr {
            if territory.eq_ignore_ascii_case(want) {
                return Some(*iana);
            }
        }
    }
    default
}

pub fn is_valid_iana(iana: &str) -> bool {
    iana.parse::<chrono_tz::Tz>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_windows_zones_map_to_iana() {
        assert_eq!(
            map_windows_zone("AUS Eastern Standard Time", None),
            Some("Australia/Sydney")
        );
        assert_eq!(
            map_windows_zone("Tokyo Standard Time", None),
            Some("Asia/Tokyo")
        );
        assert_eq!(
            map_windows_zone("GMT Standard Time", None),
            Some("Europe/London")
        );
        assert_eq!(
            map_windows_zone("Eastern Standard Time", None),
            Some("America/New_York")
        );
    }

    #[test]
    fn territory_overrides_win_then_fall_back_to_001() {
        assert_eq!(
            map_windows_zone("Eastern Standard Time", Some("CA")),
            Some("America/Toronto")
        );
        assert_eq!(
            map_windows_zone("Eastern Standard Time", Some("US")),
            Some("America/New_York")
        );
        assert_eq!(
            map_windows_zone("Eastern Standard Time", Some("ZZ")),
            Some("America/New_York")
        );
        assert_eq!(
            map_windows_zone("GMT Standard Time", Some("IE")),
            Some("Europe/Dublin")
        );
        assert_eq!(
            map_windows_zone("GMT Standard Time", Some("ZZ")),
            Some("Europe/London")
        );
        assert_eq!(
            map_windows_zone("AUS Eastern Standard Time", Some("AU")),
            Some("Australia/Sydney")
        );
    }

    #[test]
    fn unknown_key_or_empty_territory_is_unavailable_never_guessed() {
        assert_eq!(map_windows_zone("", None), None);
        assert_eq!(map_windows_zone("Nowhere Standard Time", None), None);
        assert_eq!(map_windows_zone("Nowhere Standard Time", Some("US")), None);
    }

    #[test]
    fn every_pinned_iana_name_parses() {
        for line in DATA.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let iana = line.split('\t').nth(2).unwrap();
            assert!(is_valid_iana(iana), "{iana} is not a chrono-tz zone");
        }
    }
}
