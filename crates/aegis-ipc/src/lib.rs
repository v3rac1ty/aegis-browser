//! engine event types and validation of untrusted input
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;
use url::Url;

const MAX_TITLE_CHARS: usize = 512;
const MAX_URL_BYTES: usize = 8192;
const POPUP_WINDOW_MS: u64 = 10_000;
const MAX_POPUPS_PER_WINDOW: usize = 3;

/// a parsed page url with an allowed scheme
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageUrl(Url);

impl PageUrl {
    /// return the normalized url
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// take the normalized url as a string
    pub fn into_string(self) -> String {
        self.0.to_string()
    }

    /// return the ascii host when present
    pub fn host(&self) -> Option<&str> {
        self.0.host_str()
    }
}

/// a reason a page url was rejected
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UrlError {
    /// the input exceeds the byte limit
    TooLong,
    /// the input cannot be parsed as a url
    Invalid,
    /// the scheme cannot be used for a page
    SchemeNotAllowed,
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => f.write_str("url is too long"),
            Self::Invalid => f.write_str("invalid url"),
            Self::SchemeNotAllowed => f.write_str("url scheme is not allowed"),
        }
    }
}

/// remove title characters that can hide or spoof page identity
pub fn sanitize_title(raw: &str) -> String {
    let mut title = String::new();
    let mut chars = 0;

    for ch in raw.chars() {
        let ch = match ch {
            '\t' | '\n' | '\r' => ' ',
            _ if ch.is_control() || is_hidden_format(ch) => continue,
            _ => ch,
        };

        if ch.is_whitespace() {
            if !title.is_empty() && !title.ends_with(' ') && chars < MAX_TITLE_CHARS {
                title.push(' ');
                chars += 1;
            }
            continue;
        }

        if chars == MAX_TITLE_CHARS {
            break;
        }
        title.push(ch);
        chars += 1;
    }

    title.trim_end().to_owned()
}

fn is_hidden_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{3164}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// parse and validate a top-level page url
pub fn page_url(raw: &str) -> Result<PageUrl, UrlError> {
    if raw.len() > MAX_URL_BYTES {
        return Err(UrlError::TooLong);
    }

    let parsed = Url::parse(raw).map_err(|_| UrlError::Invalid)?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(PageUrl(parsed)),
        "http" | "https" => Err(UrlError::Invalid),
        "about" if raw == "about:blank" => Ok(PageUrl(parsed)),
        _ => Err(UrlError::SchemeNotAllowed),
    }
}

/// check whether a top-level navigation is safe to open
pub fn is_allowed_navigation(raw: &str) -> bool {
    // block data and blob phishing pages and file access at the top level
    page_url(raw).is_ok()
}

/// return a host suitable for display in browser chrome
pub fn display_host(url: &PageUrl) -> String {
    let Some(host) = url.host() else {
        return "about:blank".to_owned();
    };

    // keep idn hosts in ascii to avoid homograph spoofing
    host.strip_prefix("www.").unwrap_or(host).to_owned()
}

/// limit new-window requests separately for each view
#[derive(Debug, Default)]
pub struct PopupLimiter {
    requests: HashMap<u64, Vec<u64>>,
}

impl PopupLimiter {
    /// create an empty popup limiter
    pub fn new() -> Self {
        Self::default()
    }

    /// allow up to three requests per view in ten seconds
    pub fn allow(&mut self, view: u64, now_ms: u64) -> bool {
        self.requests.retain(|_, times| {
            times.retain(|time| now_ms.saturating_sub(*time) < POPUP_WINDOW_MS);
            !times.is_empty()
        });

        let times = self.requests.entry(view).or_default();
        if times.len() == MAX_POPUPS_PER_WINDOW {
            return false;
        }

        times.push(now_ms);
        true
    }

    /// drop request history when a view closes
    pub fn forget(&mut self, view: u64) {
        self.requests.remove(&view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_titles() {
        let cases = [
            ("evil\u{202E}gpj.exe", "evilgpj.exe"),
            ("a\u{200B}b\u{200C}c\u{200D}d\u{2060}e\u{FEFF}f", "abcdef"),
            ("a\t\n\rb\u{0000}\u{001F}\u{007F}\u{0085}c", "a bc"),
            ("", ""),
            (" \t\n\r ", ""),
            ("  中文 😀  🦀  ", "中文 😀 🦀"),
            (
                "a\u{061C}\u{200E}\u{200F}\u{202A}\u{202B}\u{202C}\u{202D}\u{202E}\u{2066}\u{2067}\u{2068}\u{2069}b",
                "ab",
            ),
            ("a\u{115F}\u{1160}\u{3164}\u{FFA0}\u{180E}b", "ab"),
            (
                "a\u{2061}\u{2062}\u{2063}\u{2064}\u{E0000}\u{E0001}\u{E007F}b",
                "ab",
            ),
        ];

        for (raw, expected) in cases {
            assert_eq!(sanitize_title(raw), expected, "raw: {raw:?}");
        }
    }

    #[test]
    fn caps_titles_at_char_boundaries() {
        assert_eq!(sanitize_title(&"a".repeat(10_000)), "a".repeat(512));
        assert_eq!(sanitize_title(&"😀".repeat(10_000)), "😀".repeat(512));
        assert_eq!(
            sanitize_title(&format!("{}😀", "a".repeat(511))),
            format!("{}😀", "a".repeat(511))
        );
    }

    #[test]
    fn rejects_unsafe_urls() {
        let cases = [
            ("javascript:alert(1)", UrlError::SchemeNotAllowed),
            ("JaVaScRiPt:alert(1)", UrlError::SchemeNotAllowed),
            ("file:///etc/passwd", UrlError::SchemeNotAllowed),
            ("data:text/html,<h1>x</h1>", UrlError::SchemeNotAllowed),
            ("blob:https://x/y", UrlError::SchemeNotAllowed),
            ("ftp://example.com", UrlError::SchemeNotAllowed),
            ("about:config", UrlError::SchemeNotAllowed),
            ("http://", UrlError::Invalid),
        ];

        for (raw, expected) in cases {
            assert_eq!(page_url(raw), Err(expected), "raw: {raw}");
            assert!(!is_allowed_navigation(raw), "raw: {raw}");
        }
        assert_eq!(
            page_url(&format!("https://example.com/{}", "a".repeat(9000))),
            Err(UrlError::TooLong)
        );
    }

    #[test]
    fn accepts_and_displays_safe_urls() {
        let cases = [
            ("about:blank", "about:blank", None, "about:blank"),
            (
                "https://user:pass@example.com",
                "https://user:pass@example.com/",
                Some("example.com"),
                "example.com",
            ),
            (
                "https://www.example.com:8443/path",
                "https://www.example.com:8443/path",
                Some("www.example.com"),
                "example.com",
            ),
            (
                "https://xn--bcher-kva.example/",
                "https://xn--bcher-kva.example/",
                Some("xn--bcher-kva.example"),
                "xn--bcher-kva.example",
            ),
            (
                "HTTP://EXAMPLE.COM",
                "http://example.com/",
                Some("example.com"),
                "example.com",
            ),
        ];

        for (raw, normalized, host, shown) in cases {
            let Ok(url) = page_url(raw) else {
                panic!("expected allowed url: {raw}");
            };
            assert!(is_allowed_navigation(raw));
            assert_eq!(url.as_str(), normalized);
            assert_eq!(url.host(), host);
            assert_eq!(display_host(&url), shown);
            assert_eq!(url.into_string(), normalized);
        }
    }

    #[test]
    fn limits_popups_per_view_and_window() {
        let mut limiter = PopupLimiter::new();
        assert!(limiter.allow(1, 0));
        assert!(limiter.allow(1, 1));
        assert!(limiter.allow(1, 2));
        assert!(!limiter.allow(1, 3));
        assert!(limiter.allow(2, 3));
        assert!(limiter.allow(1, 10_000));
        assert!(limiter.allow(1, 10_001));
        assert!(limiter.allow(1, 10_002));
        assert!(!limiter.allow(1, 10_003));
        limiter.forget(1);
        assert!(limiter.allow(1, 10_003));
    }
}
