//! The update check, pure half: read the release JSON and compare versions. Pure and portable:
//! `std` and `serde_json` only. The request itself (`check` in `ui.rs` via `source::get_release`)
//! and the message box live elsewhere. Nothing taken from the response is ever opened or shown as
//! a link: the download page is the constant `RELEASES_URL`.

/// The one request, made only when the user clicks *Check for updates*.
pub const API_URL: &str = "https://api.github.com/repos/mariyaholic/mh-ticker/releases/latest";
/// What *Open the download page* opens. Never replaced by anything from the response.
pub const RELEASES_URL: &str = "https://github.com/mariyaholic/mh-ticker/releases/latest";
/// The `User-Agent` of the update request: the app's name and nothing else. No version number (it
/// would tell the server which build is asking) and no browser string (the request is not a
/// browser's). GitHub's API only requires that one is sent.
pub const USER_AGENT: &str = "MH-Ticker";

/// Is `ua` an app name only: letters, spaces and hyphens, with no digit and no browser token?
#[cfg(test)]
pub fn is_plain_app_name(ua: &str) -> bool {
    let lower = ua.to_ascii_lowercase();
    !ua.is_empty()
        && ua.chars().all(|c| c.is_ascii_alphabetic() || c == '-' || c == ' ')
        && ["mozilla", "chrome", "safari", "webkit", "windows"].iter().all(|b| !lower.contains(b))
}

/// Bytes read from the response at most.
pub const MAX_BODY: usize = 256 * 1024;

/// What the worker thread reports, shown by the message box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The running version is the latest (or newer, e.g. a local build).
    UpToDate,
    /// A newer release exists; the string is `canonical(tag)`: the parsed numbers joined by dots and
    /// nothing else from the response (`v1.9.0-rc1+x` is `1.9.0`).
    Newer(String),
    /// The request failed or the answer was not understood. Fixed text, never the response body
    /// or an error that could echo a URL: `"network"`, `"HTTP <code>"`, `"unreadable answer"`.
    Failed(String),
}

/// The `tag_name` of a GitHub "latest release" JSON document, trimmed. `None` when the text is not
/// JSON, is not an object, has no string `tag_name`, or the tag is empty, longer than 64 bytes or
/// contains a control character. Drafts and pre-releases are not served by `/releases/latest`, so
/// no other field is read.
pub fn latest(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let raw = value.get("tag_name")?.as_str()?;
    // Checked before trimming, so a tag that ends in a newline is refused rather than tidied.
    if raw.chars().any(char::is_control) {
        return None;
    }
    let tag = raw.trim();
    if tag.is_empty() || tag.len() > 64 {
        return None;
    }
    Some(tag.to_string())
}

/// Split a version into its numbers and whether it is a pre-release (a `-suffix`, as opposed to
/// a `+build` or nothing). `None` for anything that is not dotted ASCII digits before the suffix.
fn split_version(s: &str) -> Option<(Vec<u64>, bool)> {
    let s = s.strip_prefix(|c: char| c == 'v' || c == 'V').unwrap_or(s);
    let cut = s.find(['-', '+']).unwrap_or(s.len());
    let (core, suffix) = s.split_at(cut);
    let nums = core
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u64>().ok()
        })
        .collect::<Option<Vec<u64>>>()?;
    Some((nums, suffix.starts_with('-')))
}

/// Is `tag` a newer release than `current`? Both may start with `v`/`V`. Each is dotted numbers
/// (`1.8.0`), optionally followed by `-suffix` or `+build` (the suffix is everything from the first
/// `-` or `+`). Missing components count as 0, so `1.8` equals `1.8.0`. Numbers compare as numbers
/// (`1.10` > `1.9`). With equal numbers, a version with a `-suffix` (a pre-release) is older than
/// the same numbers without one; two suffixes, or only a `+build`, compare equal. `None` when
/// either side has no leading number or any component is not all ASCII digits / overflows `u64`.
pub fn newer(current: &str, tag: &str) -> Option<bool> {
    let (cur_nums, cur_pre) = split_version(current)?;
    let (tag_nums, tag_pre) = split_version(tag)?;
    for i in 0..cur_nums.len().max(tag_nums.len()) {
        let a = cur_nums.get(i).copied().unwrap_or(0);
        let b = tag_nums.get(i).copied().unwrap_or(0);
        if a != b {
            return Some(b > a);
        }
    }
    // Same numbers: only a release that is not a pre-release is newer than a pre-release.
    Some(cur_pre && !tag_pre)
}

/// The version in `tag` as plain numbers joined by dots (`v1.9.0-rc1+x` -> `1.9.0`), dropping the
/// prefix and any suffix. `None` when `split_version` refuses it. The only text from the network
/// that is ever displayed, and it can contain digits and dots only.
pub fn canonical(tag: &str) -> Option<String> {
    let (nums, _) = split_version(tag)?;
    Some(nums.iter().map(u64::to_string).collect::<Vec<_>>().join("."))
}

/// Turn a response body into the outcome shown to the user: `latest(body)` then `newer(current,
/// tag)`. `Failed("unreadable answer")` when either is `None`; `Newer(canonical(tag))` / `UpToDate` otherwise.
pub fn decide(current: &str, body: &str) -> Outcome {
    let Some(tag) = latest(body) else {
        return Outcome::Failed("unreadable answer".into());
    };
    match newer(current, &tag) {
        Some(true) => match canonical(&tag) {
            Some(v) => Outcome::Newer(v),
            None => Outcome::Failed("unreadable answer".into()),
        },
        Some(false) => Outcome::UpToDate,
        None => Outcome::Failed("unreadable answer".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_request_sends_an_app_name_only() {
        assert!(is_plain_app_name(USER_AGENT), "{USER_AGENT}");
        assert!(!USER_AGENT.chars().any(|c| c.is_ascii_digit()), "no version number");
        // The browser string the other requests carry is exactly what this check refuses.
        let browser = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        assert!(!is_plain_app_name(browser));
        assert!(!is_plain_app_name("MH-Ticker/1.9.0"));
        assert!(!is_plain_app_name(""));
    }

    #[test]
    fn update_latest_reads_the_tag_from_a_release() {
        let body = r#"{"tag_name":"v1.9.0","name":"1.9","html_url":"https://evil.example"}"#;
        assert_eq!(latest(body).as_deref(), Some("v1.9.0"));
        assert_eq!(latest(r#"{"tag_name":" v1.9.0 "}"#).as_deref(), Some("v1.9.0"));
        // A 64-byte tag is the longest accepted.
        let longest = format!(r#"{{"tag_name":"{}"}}"#, "1".repeat(64));
        assert_eq!(latest(&longest), Some("1".repeat(64)));
    }

    #[test]
    fn update_latest_refuses_junk() {
        let too_long = format!(r#"{{"tag_name":"{}"}}"#, "1".repeat(65));
        for bad in [
            "",
            "not json",
            "<html>rate limited</html>",
            "[]",
            "null",
            "{}",
            r#"{"name":"1.9.0"}"#,
            r#"{"tag_name":null}"#,
            r#"{"tag_name":5}"#,
            r#"{"tag_name":["v1.9.0"]}"#,
            r#"{"tag_name":""}"#,
            r#"{"tag_name":"   "}"#,
            r#"{"tag_name":"v1.9.0\n"}"#,
            r#"{"tag_name":"v1\n.9"}"#,
            too_long.as_str(),
        ] {
            assert_eq!(latest(bad), None, "{bad:?} should be refused");
        }
    }

    #[test]
    fn update_newer_compares_numbers_not_text() {
        assert_eq!(newer("1.8.0", "v1.9.0"), Some(true));
        assert_eq!(newer("1.9.0", "1.10.0"), Some(true));
        assert_eq!(newer("1.10", "1.9"), Some(false));
        assert_eq!(newer("1.8", "1.8.0"), Some(false));
        assert_eq!(newer("1.8.0", "1.8.0.1"), Some(true));
        assert_eq!(newer("v1.8.0", "V1.8.0"), Some(false));
        assert_eq!(newer("1.8.0", "1.7.9"), Some(false));
        // Leading zeros are the same number.
        assert_eq!(newer("1.08.0", "1.8.0"), Some(false));
        assert_eq!(newer("1.8.0", "01.08.01"), Some(true));
        assert_eq!(newer("1.9", "01.10"), Some(true));
    }

    #[test]
    fn update_newer_handles_prerelease_suffixes() {
        assert_eq!(newer("1.8.0", "1.8.0-beta"), Some(false));
        assert_eq!(newer("1.8.0-beta", "1.8.0"), Some(true));
        assert_eq!(newer("1.8.0-beta", "1.8.0-rc1"), Some(false));
        assert_eq!(newer("1.7.1", "1.8.0-beta"), Some(true));
        assert_eq!(newer("1.8.0", "1.8.0+build5"), Some(false));
        assert_eq!(newer("1.8.0+build5", "1.8.0"), Some(false));
        assert_eq!(newer("1.8.0-beta", "1.8.1-beta"), Some(true));
        assert_eq!(newer("v1.8.0", "v1.8.0-"), Some(false), "an empty suffix is still a pre-release");
    }

    #[test]
    fn update_newer_garbage_is_none() {
        for (current, tag) in [
            ("1.8.0", ""),
            ("1.8.0", "latest"),
            ("", "1.8.0"),
            ("1.8.0", "1..2"),
            ("1.8.0", "1.x"),
            ("1.8.0", "99999999999999999999999"),
            ("1.8.0", "v"),
            ("1.8.0", "vv1.9"),
            ("1.8.0", ".1"),
            ("1.8.0", "1.9 "),
            ("1.8.0", "-1.9"),
            ("1.8.0", "+1"),
        ] {
            assert_eq!(newer(current, tag), None, "newer({current:?}, {tag:?})");
        }
    }

    #[test]
    fn update_decide() {
        assert_eq!(decide("1.7.1", r#"{"tag_name":"v1.8.0"}"#), Outcome::Newer("1.8.0".into()));
        assert_eq!(decide("1.8.0", r#"{"tag_name":"v1.8.0"}"#), Outcome::UpToDate);
        assert_eq!(decide("1.8.1", r#"{"tag_name":"v1.8.0"}"#), Outcome::UpToDate, "a local build can be ahead");
        assert_eq!(decide("1.8.0", "junk"), Outcome::Failed("unreadable answer".into()));
        assert_eq!(decide("1.8.0", r#"{"tag_name":"latest"}"#), Outcome::Failed("unreadable answer".into()));
        assert_eq!(decide("1.8.0", r#"{"tag_name":7}"#), Outcome::Failed("unreadable answer".into()));
    }

    #[test]
    fn update_shows_only_the_canonical_version() {
        assert_eq!(canonical("v1.9.0-rc1+x").as_deref(), Some("1.9.0"));
        assert_eq!(canonical("V2.10").as_deref(), Some("2.10"));
        assert_eq!(canonical("latest"), None);
        assert_eq!(decide("1.8.0", r#"{"tag_name":"v9.9.9-<b>click</b>"}"#), Outcome::Newer("9.9.9".into()));
    }

    #[test]
    fn update_outcome_never_echoes_the_response() {
        // A body whose text must not reach the message box, except as the sanitised tag.
        let hostile = [
            r#"{"tag_name":"https://evil.example/x","html_url":"https://evil.example"}"#,
            r#"{"html_url":"https://evil.example","tag_name":"v1.9.0"}"#,
            r#"{"tag_name":"v1.9.0 SECRETMARKER"}"#,
            "SECRETMARKER<html>not json",
            r#"{"tag_name":"SECRETMARKER"}"#,
        ];
        for body in hostile {
            let shown = match decide("1.8.0", body) {
                Outcome::UpToDate => String::new(),
                Outcome::Newer(tag) => tag,
                Outcome::Failed(why) => why,
            };
            assert!(!shown.contains("evil"), "{shown:?}");
            assert!(!shown.contains("SECRETMARKER"), "{shown:?}");
        }
        assert_eq!(RELEASES_URL, "https://github.com/mariyaholic/mh-ticker/releases/latest");
    }
}
