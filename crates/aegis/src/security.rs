use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use aegis_engine::PermissionKind;
use aegis_shield::{HttpsExceptions, Upgrade, upgrade};

pub struct SecurityStore {
    pub exceptions: HttpsExceptions,
    hosts: HashSet<String>,
    permissions: HashMap<(String, String), bool>,
    directory: PathBuf,
}

impl SecurityStore {
    pub fn load(directory: PathBuf) -> Self {
        let mut store = Self {
            exceptions: HttpsExceptions::default(),
            hosts: HashSet::new(),
            permissions: HashMap::new(),
            directory,
        };
        if let Ok(text) = fs::read_to_string(store.directory.join("https_exceptions.json")) {
            for host in json_strings(&text) {
                store.hosts.insert(host.clone());
                store.exceptions.allow(&host);
            }
        }
        if let Ok(text) = fs::read_to_string(store.directory.join("permissions.json")) {
            for (key, allowed) in json_permissions(&text) {
                if let Some((origin, kind)) = key.rsplit_once('|') {
                    store
                        .permissions
                        .insert((origin.to_owned(), kind.to_owned()), allowed);
                }
            }
        }
        store
    }

    pub fn allow_host(&mut self, host: &str) -> Result<(), String> {
        let host = host.to_ascii_lowercase();
        self.hosts.insert(host.clone());
        self.exceptions.allow(&host);
        self.save_hosts()
    }

    pub fn clear_hosts(&mut self) -> Result<(), String> {
        self.hosts.clear();
        self.exceptions = HttpsExceptions::default();
        self.save_hosts()
    }

    fn save_hosts(&self) -> Result<(), String> {
        let mut hosts: Vec<_> = self.hosts.iter().collect();
        hosts.sort_unstable();
        let mut json = String::from("[");
        for (index, host) in hosts.into_iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            json_string(&mut json, host);
        }
        json.push(']');
        atomic_write(
            &self.directory.join("https_exceptions.json"),
            json.as_bytes(),
        )
    }

    pub fn permission(&self, origin: &str, kind: PermissionKind) -> Option<bool> {
        self.permissions
            .get(&(origin.to_owned(), permission_name(kind).to_owned()))
            .copied()
    }

    pub fn remember_permission(
        &mut self,
        origin: &str,
        kind: PermissionKind,
        allow: bool,
    ) -> Result<(), String> {
        self.permissions
            .insert((origin.to_owned(), permission_name(kind).to_owned()), allow);
        let mut rows: Vec<_> = self.permissions.iter().collect();
        rows.sort_unstable_by(|a, b| a.0.cmp(b.0));
        let mut json = String::from("{");
        for (index, ((origin, kind), allowed)) in rows.into_iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            json_string(&mut json, &format!("{origin}|{kind}"));
            json.push(':');
            json.push_str(if *allowed { "true" } else { "false" });
        }
        json.push('}');
        atomic_write(&self.directory.join("permissions.json"), json.as_bytes())
    }
}

pub fn decision(url: &str, https_only: bool, exceptions: &HttpsExceptions) -> Upgrade {
    if !https_only {
        return Upgrade::Keep;
    }
    let Upgrade::To(secure) = upgrade(url) else {
        return Upgrade::Keep;
    };
    let Some(host) = host(url) else {
        return Upgrade::Keep;
    };
    if exceptions.is_allowed(host) {
        Upgrade::Keep
    } else {
        Upgrade::To(secure)
    }
}

pub fn host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?;
    let authority = authority.rsplit('@').next()?;
    let hostname = if authority.starts_with('[') {
        authority.split_once(']')?.0.strip_prefix('[')?
    } else {
        authority.split(':').next()?
    };
    (!hostname.is_empty()).then_some(hostname)
}

pub fn permission_origin(raw: &str) -> Option<String> {
    let url = aegis_ipc::page_url(raw).ok()?;
    let normalized = url.as_str();
    let (scheme, rest) = normalized.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    Some(format!("{scheme}://{authority}"))
}

pub fn permission_name(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Camera => "camera",
        PermissionKind::Microphone => "microphone",
        PermissionKind::Location => "location",
        PermissionKind::Notifications => "notifications",
        PermissionKind::ClipboardRead => "clipboard",
        PermissionKind::Other => "other",
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let mut file = File::create(&tmp).map_err(|error| error.to_string())?;
    file.write_all(bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(tmp, path).map_err(|error| error.to_string())
}

fn json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ch if ch < ' ' => {}
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn json_strings(text: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        for ch in chars.by_ref() {
            if escaped {
                value.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                break;
            } else {
                value.push(ch);
            }
        }
        values.push(value);
    }
    values
}

fn json_permissions(text: &str) -> Vec<(String, bool)> {
    let mut result = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut key = String::new();
        let mut escaped = false;
        for ch in chars.by_ref() {
            if escaped {
                key.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                break;
            } else {
                key.push(ch);
            }
        }
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        if chars.next() != Some(':') {
            continue;
        }
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        let value: String = chars
            .by_ref()
            .take_while(|ch| ch.is_ascii_alphabetic())
            .collect();
        if value == "true" || value == "false" {
            result.push((key, value == "true"));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_decision_respects_local_exceptions_and_setting() {
        let mut exceptions = HttpsExceptions::default();
        assert_eq!(
            decision("http://example.com/a", true, &exceptions),
            Upgrade::To("https://example.com/a".to_owned())
        );
        exceptions.allow("example.com");
        assert_eq!(
            decision("http://example.com/a", true, &exceptions),
            Upgrade::Keep
        );
        assert_eq!(
            decision("http://localhost/a", true, &exceptions),
            Upgrade::Keep
        );
        assert_eq!(
            decision("http://192.168.1.1/a", true, &exceptions),
            Upgrade::Keep
        );
        assert_eq!(
            decision("http://example.org/a", false, &exceptions),
            Upgrade::Keep
        );
    }

    #[test]
    fn remembered_permission_lookup() {
        let mut store = SecurityStore {
            exceptions: HttpsExceptions::default(),
            hosts: HashSet::new(),
            permissions: HashMap::new(),
            directory: PathBuf::new(),
        };
        assert_eq!(
            store.permission("https://example.com", PermissionKind::Camera),
            None
        );
        store.permissions.insert(
            ("https://example.com".to_owned(), "camera".to_owned()),
            false,
        );
        assert_eq!(
            store.permission("https://example.com", PermissionKind::Camera),
            Some(false)
        );
        assert_eq!(
            json_permissions(r#"{"https://example.com|camera":true}"#),
            vec![(String::from("https://example.com|camera"), true)]
        );
    }

    #[test]
    fn permission_origins_drop_paths_and_default_ports() {
        assert_eq!(
            permission_origin("https://EXAMPLE.com:443/a?q=1"),
            Some("https://example.com".to_owned())
        );
        assert_eq!(
            permission_origin("http://example.com:8080/a"),
            Some("http://example.com:8080".to_owned())
        );
        assert_eq!(permission_origin("about:blank"), None);
        assert_eq!(permission_origin("file:///tmp/page"), None);
    }
}
