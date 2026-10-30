use serde::{Deserialize, Serialize};

/// tab layout in the app
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// sidebar layout
    #[default]
    Vertical,
    /// tab strip layout
    Horizontal,
}

/// built-in search providers
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    /// duckduckgo search
    #[default]
    DuckDuckGo,
    /// brave search
    Brave,
    /// startpage search
    Startpage,
    /// google search
    Google,
    /// bing search
    Bing,
}

impl SearchEngine {
    /// returns a search URL with the query encoded as UTF-8 bytes
    pub fn query_url(&self, query: &str) -> String {
        let base = match self {
            Self::DuckDuckGo => "https://duckduckgo.com/?q=",
            Self::Brave => "https://search.brave.com/search?q=",
            Self::Startpage => "https://www.startpage.com/sp/search?query=",
            Self::Google => "https://www.google.com/search?q=",
            Self::Bing => "https://www.bing.com/search?q=",
        };
        let mut url = String::from(base);
        for byte in query.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                url.push(char::from(byte));
            } else {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                url.push('%');
                url.push(char::from(HEX[usize::from(byte >> 4)]));
                url.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
        url
    }
}

/// browser preferences saved in the session
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// tab layout
    pub layout: Layout,
    /// compact sidebar mode
    pub compact: bool,
    /// idle minutes before suspension
    pub suspend_after_minutes: u32,
    /// idle minutes before discarding
    pub discard_after_minutes: u32,
    /// search provider
    pub search_engine: SearchEngine,
    /// upgrade plain HTTP navigation
    pub https_only: bool,
    /// block known trackers
    pub block_trackers: bool,
    /// block third-party cookies
    pub block_third_party_cookies: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            layout: Layout::Vertical,
            compact: false,
            suspend_after_minutes: 10,
            discard_after_minutes: 60,
            search_engine: SearchEngine::DuckDuckGo,
            https_only: true,
            block_trackers: true,
            block_third_party_cookies: true,
        }
    }
}

impl Settings {
    pub(crate) fn normalized(mut self) -> Self {
        self.suspend_after_minutes = self.suspend_after_minutes.max(1);
        self.discard_after_minutes = self.discard_after_minutes.max(self.suspend_after_minutes);
        self
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding() {
        assert_eq!(
            SearchEngine::DuckDuckGo.query_url("a b&#é"),
            "https://duckduckgo.com/?q=a%20b%26%23%C3%A9"
        );
        assert_eq!(
            SearchEngine::Brave.query_url("azAZ09-_.~"),
            "https://search.brave.com/search?q=azAZ09-_.~"
        );
    }

    #[test]
    fn old_settings_use_defaults() {
        let settings: Settings = serde_json::from_str("{\"compact\":true}").unwrap();
        assert!(settings.compact);
        assert_eq!(settings.suspend_after_minutes, 10);
    }
}
