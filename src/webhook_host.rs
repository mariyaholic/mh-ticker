//! Display only a parsed webhook host, never credentials or path/query/fragment secrets.

pub(crate) fn redact(raw: &str) -> String {
    // Reject shorthand such as `https:SECRET`, which a browser URL parser can
    // reinterpret as a host. Error messages must not echo malformed secrets.
    if !raw.split_once("://").is_some_and(|(scheme, _)|
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return "invalid webhook URL".into();
    }
    let Ok(url) = url::Url::parse(raw) else {
        return "invalid webhook URL".into();
    };
    if !matches!(url.scheme(), "http" | "https") {
        return "invalid webhook URL".into();
    }
    let Some(host) = url.host() else {
        return "invalid webhook URL".into();
    };
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn credentials_and_url_suffixes_never_reach_display() {
        for i in 0..100_000 {
            let secret = format!("SECRET-{i}");
            for raw in [
                format!("https://user:{secret}@example.com/path"),
                format!("https://example.com?token={secret}"),
                format!("https://example.com#{secret}"),
            ] {
                assert_eq!(redact(&raw), "example.com");
            }
        }
        assert_eq!(redact("https://user:SECRET@[::1]:8443/path"), "[::1]:8443");
        assert_eq!(redact("https://user:SECRET@[::1]:443/path"), "[::1]");
        assert_eq!(redact("https://discord.com/api/webhooks/SECRET"), "discord.com");
    }

    #[test]
    fn malformed_urls_never_echo_the_input() {
        for raw in ["SECRET", "https:SECRET", "https://user:SECRET", "mailto:SECRET", "data:text/plain,SECRET", ""] {
            assert_eq!(redact(raw), "invalid webhook URL");
        }
    }
}
