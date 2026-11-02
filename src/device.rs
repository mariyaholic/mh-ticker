// SPDX-License-Identifier: MIT
//! Windows device-zone adapter.
//!
//! Reads the current dynamic time-zone key and the user's territory, then maps
//! the key through the pinned CLDR table in `mh_session::zones`. A failed
//! mapping is surfaced as an unavailable device row: it never guesses.

use mh_session::view::{DeviceInfo, DeviceSource};
use mh_session::zones::{is_valid_iana, map_windows_zone};

use crate::config::SessionConfig;

/// Build the one device row's input. A configured IANA zone wins, but the row
/// is labelled "configured" rather than "auto-detected".
pub fn current(cfg: &SessionConfig) -> DeviceInfo {
    let (windows_id, territory, dst_disabled) = probe();
    row_from(windows_id, territory, dst_disabled, cfg)
}

fn row_from(
    windows_id: String,
    territory: Option<String>,
    dst_disabled: bool,
    cfg: &SessionConfig,
) -> DeviceInfo {
    if let Some(zone) = cfg
        .device_zone
        .as_deref()
        .map(str::trim)
        .filter(|z| !z.is_empty())
    {
        if is_valid_iana(zone) {
            return DeviceInfo {
                iana: Some(zone.to_string()),
                source: DeviceSource::Configured,
                windows_id: Some(windows_id).filter(|s| !s.is_empty()),
                territory,
            };
        }
    }
    // 02-L3 (Manager ruling): with "Adjust for daylight saving time
    // automatically" off, Windows shows standard time all year while the IANA
    // key still resolves through the full DST table — an "auto-detected" row
    // would be an hour off for half the year. The row is reported unavailable
    // rather than guessed.
    if dst_disabled {
        return DeviceInfo::unavailable(Some(windows_id).filter(|s| !s.is_empty()), territory);
    }
    match map_windows_zone(&windows_id, territory.as_deref()) {
        Some(iana) => DeviceInfo {
            iana: Some(iana.to_string()),
            source: DeviceSource::Auto,
            windows_id: Some(windows_id),
            territory,
        },
        None => DeviceInfo::unavailable(Some(windows_id).filter(|s| !s.is_empty()), territory),
    }
}

#[cfg(windows)]
fn probe() -> (String, Option<String>, bool) {
    use windows_sys::Win32::Globalization::{GetUserDefaultGeoName, GetUserDefaultLocaleName};
    use windows_sys::Win32::System::Time::{
        GetDynamicTimeZoneInformation, DYNAMIC_TIME_ZONE_INFORMATION,
    };

    let mut dtzi: DYNAMIC_TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let rc = unsafe { GetDynamicTimeZoneInformation(&mut dtzi) };
    if rc == u32::MAX {
        return (String::new(), None, false);
    }
    let key = from_wide(&dtzi.TimeZoneKeyName);
    let dst_disabled = dtzi.DynamicDaylightTimeDisabled != 0;

    let mut geo = [0u16; 16];
    let n = unsafe { GetUserDefaultGeoName(geo.as_mut_ptr(), geo.len() as i32) };
    let mut territory = if n > 0 { Some(from_wide(&geo)) } else { None };
    if territory.as_deref().is_none_or(|t| t.len() != 2) {
        let mut locale = [0u16; 85];
        let n = unsafe { GetUserDefaultLocaleName(locale.as_mut_ptr(), locale.len() as i32) };
        territory = if n > 0 {
            from_wide(&locale)
                .rsplit('-')
                .next()
                .filter(|t| t.len() == 2)
                .map(str::to_string)
        } else {
            None
        };
    }
    (key, territory.map(|t| t.to_ascii_uppercase()), dst_disabled)
}

#[cfg(not(windows))]
fn probe() -> (String, Option<String>, bool) {
    (String::new(), None, false)
}

#[cfg(windows)]
fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_zone_is_labelled_configured() {
        let cfg = SessionConfig {
            device_zone: Some("Europe/Berlin".into()),
            ..SessionConfig::default()
        };
        let d = current(&cfg);
        assert_eq!(d.iana.as_deref(), Some("Europe/Berlin"));
        assert_eq!(d.source, DeviceSource::Configured);
    }

    #[test]
    fn invalid_configured_zone_falls_back_to_detection_not_a_guess() {
        let cfg = SessionConfig {
            device_zone: Some("Not/AZone".into()),
            ..SessionConfig::default()
        };
        let d = current(&cfg);
        assert_ne!(d.source, DeviceSource::Configured);
        // 02-M3: the weak `is_none_or(is_valid_iana)` assertion held for every
        // possible result — and the earlier is_none_or checked the logic. What
        // must hold: this Windows box maps its real dynamic time-zone key, or
        // the row is unavailable; never a valid-but-unmapped guess.
        if let Some(iana) = d.iana.as_deref() {
            assert_eq!(d.source, DeviceSource::Auto);
            assert!(is_valid_iana(iana), "{iana} must be a real IANA zone");
        } else {
            assert_eq!(d.source, DeviceSource::Unavailable);
        }
    }

    #[test]
    fn dst_auto_adjust_off_reports_unavailable_not_the_dst_table() {
        // 02-L3 (Manager ruling): with the switch off the system shows
        // standard time, so an "auto-detected" row reading the DST table is
        // wrong; the row is unavailable instead.
        let cfg = SessionConfig::default();
        let d = row_from(
            "W. Europe Standard Time".into(),
            Some("DE".into()),
            true,
            &cfg,
        );
        assert_eq!(d.source, DeviceSource::Unavailable);
        assert!(d.iana.is_none());
        // The same key with the switch on still maps through the adapter.
        let on = row_from(
            "W. Europe Standard Time".into(),
            Some("DE".into()),
            false,
            &cfg,
        );
        assert_eq!(on.iana.as_deref(), Some("Europe/Berlin"));
        assert_eq!(on.source, DeviceSource::Auto);
        // A configured zone keeps priority over the switch (it is pinned, not
        // auto-detected).
        let pinned = SessionConfig {
            device_zone: Some("Asia/Tokyo".into()),
            ..SessionConfig::default()
        };
        let p = row_from(
            "W. Europe Standard Time".into(),
            Some("DE".into()),
            true,
            &pinned,
        );
        assert_eq!(p.iana.as_deref(), Some("Asia/Tokyo"));
        assert_eq!(p.source, DeviceSource::Configured);
    }

    #[test]
    fn the_mapping_function_is_the_real_adapters_core() {
        // 02-M3: pins the table through adapter code with a known pair, rather
        // than asserting any string is either absent or valid.
        assert_eq!(
            map_windows_zone("W. Europe Standard Time", Some("DE")),
            Some("Europe/Berlin")
        );
        assert_eq!(map_windows_zone("Not/ARealKey", None), None);
    }
}
