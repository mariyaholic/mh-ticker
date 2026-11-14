//! The diagnostics report (*Copy diagnostics report*): what the app knows about itself, as plain
//! text a person can paste into a message. Pure and portable: `std` plus the pure `probe` and
//! `webhook_host` modules. Nothing leaves the machine by itself; the text goes to the clipboard on a
//! click and the person decides who gets it.
//!
//! Two things are never in it: a webhook URL (only its host) and the user's name (the settings
//! path is scrubbed). Everything a field holds is made of names, numbers and fixed words.

use crate::probe::{Finding, Verdict};

/// What the report says, collected by the caller on the UI thread before the checks run.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub version: String,
    /// `Classic` or `Ultra`.
    pub build: String,
    pub dpi: u32,
    /// The placement's name as the menu shows it.
    pub placement: String,
    /// `strip` or `panel`.
    pub mode: String,
    pub symbols: usize,
    pub alerts: usize,
    /// The hotkey as configured (`Ctrl+Alt+M`), or `None` for off.
    pub hotkey: Option<String>,
    pub hotkey_failed: bool,
    pub autostart: bool,
    /// The settings file's path as the app has it; it is scrubbed here.
    pub settings_path: String,
    /// The user's name, when known, to scrub from anywhere it appears.
    pub user: Option<String>,
    pub save_failed: bool,
    /// The name (never the folder) a settings file that would not parse was kept under.
    pub kept_config: Option<String>,
    /// The webhook URL. Only its host is ever written.
    pub webhook: Option<String>,
    pub fallback: bool,
    pub poll_s: f64,
    pub stale_after_s: f64,
    /// The data meter's line (`Data: 3.1 MB this hour, 41 MB since start`).
    pub meter: String,
    /// The findings of the checks, in order.
    pub findings: Vec<Finding>,
}

/// The path with the user's name taken out: the folder after `Users` (or `home`) becomes
/// `%USERNAME%`, and `user`, when given, is replaced wherever it appears (case-insensitively).
pub fn scrub_path(path: &str, user: Option<&str>) -> String {
    let mut parts: Vec<String> = path.split_inclusive(['\\', '/']).map(str::to_string).collect();
    for i in 0..parts.len().saturating_sub(1) {
        let name = parts[i].trim_end_matches(['\\', '/']);
        if name.eq_ignore_ascii_case("users") || name.eq_ignore_ascii_case("home") || name.eq_ignore_ascii_case("documents and settings") {
            let sep: String = parts[i + 1].chars().filter(|c| *c == '\\' || *c == '/').collect();
            parts[i + 1] = format!("%USERNAME%{sep}");
        }
    }
    let mut out: String = parts.concat();
    if let Some(user) = user.map(str::trim).filter(|u| u.len() >= 2) {
        let lower = out.to_lowercase();
        let needle = user.to_lowercase();
        if lower.len() == out.len() {
            let mut result = String::new();
            let mut at = 0;
            while let Some(i) = lower[at..].find(&needle) {
                result.push_str(&out[at..at + i]);
                result.push_str("%USERNAME%");
                at += i + needle.len();
            }
            result.push_str(&out[at..]);
            out = result;
        }
    }
    out
}

/// The report. Order and wording are fixed, so two reports can be compared line by line.
pub fn text(f: &Facts) -> String {
    let yes = |b: bool| if b { "yes" } else { "no" };
    let hotkey = match (&f.hotkey, f.hotkey_failed) {
        (None, _) => "off".to_string(),
        (Some(k), false) => format!("{} (registered)", one_line(k, 32)),
        (Some(k), true) => format!("{} (could not be registered)", one_line(k, 32)),
    };
    let webhook = match f.webhook.as_deref().filter(|w| !w.trim().is_empty()) {
        None => "none".to_string(),
        Some(url) => format!("set, host {}", crate::webhook_host::redact(url)),
    };
    let warns = f.findings.iter().filter(|x| x.verdict == Verdict::Warn).count();
    let mut lines = vec![
        "MH Ticker diagnostics report".to_string(),
        format!("Version: {} ({})", one_line(&f.version, 24), one_line(&f.build, 16)),
        format!("Window: {}, {} DPI, placement {}", one_line(&f.mode, 16), f.dpi, one_line(&f.placement, 32)),
        format!("Symbols shown: {}; alerts set: {}", f.symbols, f.alerts),
        format!("Settings file: {}", scrub_path(&one_line(&f.settings_path, 260), f.user.as_deref())),
        format!("Settings last saved: {}", if f.save_failed { "FAILED" } else { "ok" }),
        format!("Unreadable settings kept: {}", f.kept_config.as_deref().map_or("no".to_string(), |n| one_line(n, 80))),
        format!("Hotkey: {hotkey}"),
        format!("Start with Windows: {}", yes(f.autostart)),
        format!("Webhook: {webhook}"),
        format!("Backup price sources: {}", if f.fallback { "on" } else { "off" }),
        format!("Poll every {:.0} s; a quote is stale after {:.0} s", f.poll_s, f.stale_after_s),
        one_line(&f.meter, 120),
        String::new(),
        format!("Checks ({} warning{}):", warns, if warns == 1 { "" } else { "s" }),
    ];
    lines.push(if f.findings.is_empty() { "none ran".to_string() } else { crate::probe::render(&f.findings) });
    lines.join("\r\n")
}

/// `s` on one line: control characters dropped, cut to `max` characters.
fn one_line(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::Verdict;

    const WEBHOOK: &str = "https://discord.com/api/webhooks/1234567890/SECRETTOKEN-abc?wait=true&thread_id=99";

    fn fixture() -> Facts {
        Facts {
            version: "1.9.0".into(),
            build: "Ultra".into(),
            dpi: 144,
            placement: "Auto".into(),
            mode: "strip".into(),
            symbols: 7,
            alerts: 2,
            hotkey: Some("Ctrl+Alt+M".into()),
            hotkey_failed: false,
            autostart: true,
            settings_path: r"C:\Users\Jane Example\AppData\Roaming\MH Ticker Ultra\settings.json".into(),
            user: Some("Jane Example".into()),
            save_failed: false,
            kept_config: None,
            webhook: Some(WEBHOOK.into()),
            fallback: false,
            poll_s: 3.0,
            stale_after_s: 90.0,
            meter: "Data: 3.1 MB this hour, 41 MB since start".into(),
            findings: vec![
                Finding { id: "quote-fields", verdict: Verdict::Pass, text: "7 shown symbols".into() },
                Finding { id: "volume", verdict: Verdict::Warn, text: "none of 150 bars carries a volume".into() },
            ],
        }
    }

    #[test]
    fn the_webhook_shows_as_its_host_only() {
        let text = text(&fixture());
        assert!(text.contains("Webhook: set, host discord.com"), "{text}");
        for secret in ["api/webhooks", "1234567890", "SECRETTOKEN", "wait=true", "thread_id", "https://", "?"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        // Not set, and a URL that does not parse.
        let mut f = fixture();
        f.webhook = None;
        assert!(super::text(&f).contains("Webhook: none"));
        f.webhook = Some("https:SECRET".into());
        let t = super::text(&f);
        assert!(!t.contains("SECRET"), "{t}");
    }

    #[test]
    fn the_settings_path_carries_no_user_name() {
        let t = text(&fixture());
        assert!(!t.contains("Jane") && !t.contains("Example"), "{t}");
        assert!(t.contains(r"Settings file: C:\Users\%USERNAME%\AppData\Roaming\MH Ticker Ultra\settings.json"), "{t}");
        // With no user name known the folder after Users is still replaced, on both separators.
        assert_eq!(scrub_path(r"C:\Users\Zed\AppData\x.json", None), r"C:\Users\%USERNAME%\AppData\x.json");
        assert_eq!(scrub_path("/home/zed/.config/x.json", None), "/home/%USERNAME%/.config/x.json");
        assert_eq!(scrub_path(r"D:\Data\zed\mh\settings.json", Some("ZED")), r"D:\Data\%USERNAME%\mh\settings.json");
        assert_eq!(scrub_path(r"C:\Program Files\x", None), r"C:\Program Files\x");
        // A one-letter name is not searched for anywhere (it would eat every letter).
        assert_eq!(scrub_path(r"C:\Data\a\b", Some("a")), r"C:\Data\a\b");
    }

    #[test]
    fn the_report_carries_the_facts_the_meter_and_the_checks() {
        let t = text(&fixture());
        for want in [
            "Version: 1.9.0 (Ultra)",
            "144 DPI",
            "Symbols shown: 7; alerts set: 2",
            "Hotkey: Ctrl+Alt+M (registered)",
            "Start with Windows: yes",
            "Data: 3.1 MB this hour, 41 MB since start",
            "Checks (1 warning):",
            "PASS  quote-fields",
            "WARN  volume",
        ] {
            assert!(t.contains(want), "missing {want:?}: {t}");
        }
        let mut f = fixture();
        f.hotkey = None;
        f.save_failed = true;
        f.kept_config = Some("settings.bad.json".into());
        f.findings.clear();
        let t = text(&f);
        assert!(t.contains("Hotkey: off") && t.contains("last saved: FAILED") && t.contains("kept: settings.bad.json") && t.contains("none ran"), "{t}");
    }
}
