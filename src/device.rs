// SPDX-License-Identifier: Apache-2.0
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
    let (windows_id, territory) = probe();
    if let Some(zone) = cfg.device_zone.as_deref().map(str::trim).filter(|z| !z.is_empty()) {
        if is_valid_iana(zone) {
            return DeviceInfo {
                iana: Some(zone.to_string()),
                source: DeviceSource::Configured,
                windows_id: Some(windows_id).filter(|s| !s.is_empty()),
                territory,
            };
        }
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
fn probe() -> (String, Option<String>) {
    use windows_sys::Win32::Globalization::{GetUserDefaultGeoName, GetUserDefaultLocaleName};
    use windows_sys::Win32::System::Time::{GetDynamicTimeZoneInformation, DYNAMIC_TIME_ZONE_INFORMATION};

    let mut dtzi: DYNAMIC_TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let rc = unsafe { GetDynamicTimeZoneInformation(&mut dtzi) };
    if rc == u32::MAX {
        return (String::new(), None);
    }
    let key = from_wide(&dtzi.TimeZoneKeyName);

    let mut geo = [0u16; 16];
    let n = unsafe { GetUserDefaultGeoName(geo.as_mut_ptr(), geo.len() as i32) };
    let mut territory = if n > 0 { Some(from_wide(&geo)) } else { None };
    if territory.as_deref().is_none_or(|t| t.len() != 2) {
        let mut locale = [0u16; 85];
        let n = unsafe { GetUserDefaultLocaleName(locale.as_mut_ptr(), locale.len() as i32) };
        territory = if n > 0 {
            from_wide(&locale).rsplit('-').next().filter(|t| t.len() == 2).map(str::to_string)
        } else {
            None
        };
    }
    (key, territory.map(|t| t.to_ascii_uppercase()))
}

#[cfg(not(windows))]
fn probe() -> (String, Option<String>) {
    (String::new(), None)
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
        let cfg = SessionConfig { device_zone: Some("Europe/Berlin".into()), ..SessionConfig::default() };
        let d = current(&cfg);
        assert_eq!(d.iana.as_deref(), Some("Europe/Berlin"));
        assert_eq!(d.source, DeviceSource::Configured);
    }

    #[test]
    fn invalid_configured_zone_falls_back_to_detection_not_a_guess() {
        let cfg = SessionConfig { device_zone: Some("Not/AZone".into()), ..SessionConfig::default() };
        let d = current(&cfg);
        assert_ne!(d.source, DeviceSource::Configured);
        assert!(d.iana.as_deref().is_none_or(is_valid_iana));
    }
}
