//! ad blocking and HTTPS upgrades
#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use adblock::content_blocking::{CbRule, CbType};
use adblock::lists::ParseOptions;
use adblock::request::Request;
use adblock::{Engine, FilterSet};
use serde::{Deserialize, Serialize};
use url::{Host, Url};

const MAX_CONTENT_RULES: usize = 140_000;

#[derive(Debug)]
pub enum ShieldError {
    Io(std::io::Error),
    Conversion,
    Json(serde_json::Error),
    TooManyExceptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceKind {
    Document,
    Subdocument,
    Script,
    Stylesheet,
    Image,
    Font,
    Media,
    Xhr,
    Websocket,
    Ping,
    Other,
}

impl ResourceKind {
    fn request_type(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Subdocument => "subdocument",
            Self::Script => "script",
            Self::Stylesheet => "stylesheet",
            Self::Image => "image",
            Self::Font => "font",
            Self::Media => "media",
            Self::Xhr => "xmlhttprequest",
            Self::Websocket => "websocket",
            Self::Ping => "ping",
            Self::Other => "other",
        }
    }
}

pub struct Shield {
    engine: Engine,
}

#[derive(Debug)]
pub struct LoadedLists {
    pub easylist: String,
    pub easyprivacy: String,
}

impl LoadedLists {
    pub fn as_strs(&self) -> [&str; 2] {
        [&self.easylist, &self.easyprivacy]
    }
}

impl Shield {
    pub fn from_lists(lists: &[&str]) -> Self {
        let mut filters = FilterSet::new(false);
        for list in lists {
            filters.add_filter_list((*list).to_owned(), ParseOptions::default());
        }
        Self {
            engine: Engine::new_with_filter_set(filters),
        }
    }

    pub fn from_loaded(lists: &LoadedLists) -> Self {
        Self::from_lists(&lists.as_strs())
    }

    /// loads list text from the resources directory
    pub fn load_default() -> Result<LoadedLists, ShieldError> {
        let exe = std::env::current_exe().map_err(ShieldError::Io)?;
        let env_dir = std::env::var_os("AEGIS_RESOURCES").map(PathBuf::from);
        let candidates = resource_candidates(
            env_dir.as_deref(),
            &exe,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            cfg!(debug_assertions),
        );
        let mut last_error = None;
        for root in candidates {
            let easylist = fs::read_to_string(root.join("filters/easylist.txt"));
            let easyprivacy = fs::read_to_string(root.join("filters/easyprivacy.txt"));
            match (easylist, easyprivacy) {
                (Ok(easylist), Ok(easyprivacy)) => {
                    return Ok(LoadedLists {
                        easylist,
                        easyprivacy,
                    });
                }
                (Err(error), _) | (_, Err(error)) => last_error = Some(error),
            }
        }
        Err(ShieldError::Io(last_error.unwrap_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "filter lists not found")
        })))
    }

    pub fn should_block(&self, url: &str, source_url: &str, kind: ResourceKind) -> bool {
        if kind == ResourceKind::Document {
            return false;
        }
        let Ok(request) = Request::new(url, source_url, kind.request_type(), "get") else {
            return false;
        };
        self.engine.check_network_request(&request).should_block()
    }

    /// returns host-specific hiding CSS; generic class and id rules need page DOM data
    pub fn cosmetic_css(&self, page_url: &str) -> Option<String> {
        let resources = self.engine.url_cosmetic_resources(page_url);
        if resources.hide_selectors.is_empty() {
            return None;
        }
        let mut selectors: Vec<_> = resources.hide_selectors.into_iter().collect();
        selectors.sort_unstable();
        Some(format!("{}{{display:none!important}}", selectors.join(",")))
    }
}

/// returns separate WebKit lists, each capped at 140,000 rules
pub fn content_blocker_json(lists: &[&str]) -> Result<Vec<String>, ShieldError> {
    let mut filters = FilterSet::new(true);
    for list in lists {
        filters.add_filter_list((*list).to_owned(), ParseOptions::default());
    }
    let (rules, _) = filters
        .into_content_blocking()
        .map_err(|_| ShieldError::Conversion)?;
    split_content_rules(rules, MAX_CONTENT_RULES)
}

fn split_content_rules(rules: Vec<CbRule>, limit: usize) -> Result<Vec<String>, ShieldError> {
    let mut exceptions = Vec::new();
    let mut ordinary = Vec::new();
    for rule in rules {
        if matches!(rule.action.typ, CbType::IgnorePreviousRules) {
            exceptions.push(rule);
        } else {
            ordinary.push(rule);
        }
    }
    if exceptions.len() >= limit {
        return Err(ShieldError::TooManyExceptions);
    }
    let mut result = Vec::new();
    for chunk in ordinary.chunks(limit - exceptions.len()) {
        let mut list = Vec::with_capacity(chunk.len() + exceptions.len());
        list.extend_from_slice(chunk);
        list.extend_from_slice(&exceptions);
        result.push(serde_json::to_string(&list).map_err(ShieldError::Json)?);
    }
    if result.is_empty() {
        result.push(serde_json::to_string(&exceptions).map_err(ShieldError::Json)?);
    }
    Ok(result)
}

/// gives resource directories in precedence order without touching the file system
pub fn resource_candidates(
    env_dir: Option<&Path>,
    exe_path: &Path,
    manifest_dir: &Path,
    debug: bool,
) -> Vec<PathBuf> {
    if let Some(dir) = env_dir {
        return vec![dir.to_path_buf()];
    }
    let mut paths = Vec::new();
    if let Some(parent) = exe_path.parent() {
        paths.push(parent.join("resources"));
    }
    if debug {
        paths.push(manifest_dir.join("../../resources"));
    }
    paths
}

impl std::fmt::Debug for Shield {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shield").finish_non_exhaustive()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Upgrade {
    Keep,
    To(String),
}

pub fn upgrade(url: &str) -> Upgrade {
    let Ok(mut parsed) = Url::parse(url) else {
        return Upgrade::Keep;
    };
    if parsed.scheme() != "http" || is_local_host(&parsed) {
        return Upgrade::Keep;
    }
    if parsed.set_scheme("https").is_err() {
        return Upgrade::Keep;
    }
    Upgrade::To(parsed.into())
}

fn is_local_host(url: &Url) -> bool {
    // local hosts often serve plain HTTP for development and router setup
    match url.host() {
        Some(Host::Domain(host)) => {
            host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local")
        }
        Some(Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HttpsExceptions {
    hosts: HashSet<String>,
}

impl HttpsExceptions {
    pub fn allow(&mut self, host: &str) {
        self.hosts.insert(host.to_ascii_lowercase());
    }

    pub fn is_allowed(&self, host: &str) -> bool {
        self.hosts.contains(&host.to_ascii_lowercase())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn candidate_precedence() {
        let exe = Path::new("/app/aegis.exe");
        let manifest = Path::new("/src/crates/aegis-shield");
        assert_eq!(
            resource_candidates(Some(Path::new("/custom")), exe, manifest, true),
            vec![PathBuf::from("/custom")]
        );
        assert_eq!(
            resource_candidates(None, exe, manifest, false),
            vec![PathBuf::from("/app/resources")]
        );
        assert_eq!(resource_candidates(None, exe, manifest, true).len(), 2);
    }

    #[test]
    fn blocking() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/filters/easyprivacy.txt");
        let list = fs::read_to_string(path).unwrap();
        let shield = Shield::from_lists(&[&list]);
        let tracker = "https://www.google-analytics.com/analytics.js";
        let source = "https://example.com/";
        assert!(shield.should_block(tracker, source, ResourceKind::Script));
        assert!(!shield.should_block(tracker, source, ResourceKind::Document));
        assert!(!shield.should_block("https://example.com/main.js", source, ResourceKind::Script));
    }

    #[test]
    fn upgrades() {
        assert_eq!(
            upgrade("http://example.com:8080/a?b=1#c"),
            Upgrade::To("https://example.com:8080/a?b=1#c".to_owned())
        );
        for url in [
            "https://example.com/",
            "http://localhost:8080/",
            "http://sub.localhost/",
            "http://router.local/",
            "http://127.0.0.1/",
            "http://[::1]/",
            "http://10.2.3.4/",
            "http://172.16.0.1/",
            "http://192.168.1.1/",
        ] {
            assert_eq!(upgrade(url), Upgrade::Keep, "{url}");
        }
        assert_eq!(
            upgrade("http://172.32.0.1/"),
            Upgrade::To("https://172.32.0.1/".into())
        );
    }

    #[test]
    fn exceptions_are_exact_and_case_insensitive() {
        let mut exceptions = HttpsExceptions::default();
        exceptions.allow("Example.COM");
        assert!(exceptions.is_allowed("example.com"));
        assert!(!exceptions.is_allowed("sub.example.com"));
        let json = serde_json::to_string(&exceptions).unwrap();
        let restored: HttpsExceptions = serde_json::from_str(&json).unwrap();
        assert!(restored.is_allowed("EXAMPLE.COM"));
    }

    #[test]
    fn content_rules_are_valid_json() {
        let text = "||tracker.example^$script\nexample.com##.ad";
        let shield = Shield::from_lists(&[text]);
        let lists = content_blocker_json(&[text]).unwrap();
        let rules: Vec<CbRule> = serde_json::from_str(&lists[0]).unwrap();
        assert!(!rules.is_empty());
        assert_eq!(
            shield.cosmetic_css("https://example.com/"),
            Some(".ad{display:none!important}".to_owned())
        );
    }

    #[test]
    fn content_rules_split() {
        let mut filters = FilterSet::new(true);
        filters.add_filter_list("||tracker.example^$script".into(), ParseOptions::default());
        let (rules, _) = filters.into_content_blocking().unwrap();
        let mut large = Vec::with_capacity(MAX_CONTENT_RULES + 1);
        for _ in 0..MAX_CONTENT_RULES + 1 {
            large.push(rules[0].clone());
        }
        large.push(rules[1].clone());
        let lists = split_content_rules(large, MAX_CONTENT_RULES).unwrap();
        assert_eq!(lists.len(), 2);
        for list in lists {
            let parsed: Vec<CbRule> = serde_json::from_str(&list).unwrap();
            assert!(parsed.len() <= MAX_CONTENT_RULES);
            assert!(matches!(
                parsed.last().unwrap().action.typ,
                CbType::IgnorePreviousRules
            ));
        }
    }

    #[test]
    #[ignore = "reports resident size and converted rule count"]
    fn measure_full_lists() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/filters");
        let loaded = LoadedLists {
            easylist: fs::read_to_string(root.join("easylist.txt")).unwrap(),
            easyprivacy: fs::read_to_string(root.join("easyprivacy.txt")).unwrap(),
        };
        let raw_bytes = loaded.easylist.len() + loaded.easyprivacy.len();
        let before = process_memory_bytes();
        let shield = Shield::from_loaded(&loaded);
        let after_engine = process_memory_bytes();
        let json = content_blocker_json(&loaded.as_strs()).unwrap();
        let rule_count: usize = json
            .iter()
            .map(|list| serde_json::from_str::<Vec<CbRule>>(list).unwrap().len())
            .sum();
        let list_count = json.len();
        drop(json);
        drop(loaded);
        let after_drop = process_memory_bytes();
        println!("raw list bytes: {raw_bytes}");
        println!(
            "engine resident delta before dropping text: {} bytes",
            after_engine.saturating_sub(before)
        );
        println!(
            "engine resident delta after dropping text: {} bytes",
            after_drop.saturating_sub(before)
        );
        println!("WebKit rule count: {rule_count}, lists: {list_count}");
        std::hint::black_box(shield);
    }

    #[cfg(target_os = "windows")]
    fn process_memory_bytes() -> usize {
        let command = format!("(Get-Process -Id {}).WorkingSet64", std::process::id());
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &command])
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    #[cfg(not(target_os = "windows"))]
    fn process_memory_bytes() -> usize {
        0
    }
}
