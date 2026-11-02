use std::io::Cursor;

use aegis_ipc::{page_url, sanitize_title};
use plist::Value as PlistValue;
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::{ImportError, MAX_DEPTH, MAX_ENTRIES, check_file_size};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedBookmarks {
    pub folders: Vec<ImportedFolder>,
    pub skipped_invalid: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedFolder {
    pub name: String,
    pub items: Vec<ImportedItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportedItem {
    Link { title: String, url: String },
    Folder(ImportedFolder),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TabPlan {
    pub groups: Vec<PlannedGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedGroup {
    pub name: String,
    pub tabs: Vec<PlannedTab>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedTab {
    pub title: String,
    pub url: String,
    pub loaded: bool,
}

#[derive(Default)]
struct Budget {
    entries: usize,
    skipped_invalid: usize,
}

impl Budget {
    fn claim(&mut self) -> bool {
        if self.entries == MAX_ENTRIES {
            return false;
        }
        self.entries += 1;
        true
    }

    fn link(&mut self, title: &str, raw_url: &str) -> Option<ImportedItem> {
        if self.entries == MAX_ENTRIES {
            return None;
        }
        let Ok(url) = page_url(raw_url) else {
            self.skipped_invalid += 1;
            return None;
        };
        self.claim();
        Some(ImportedItem::Link {
            title: sanitize_title(title),
            url: url.into_string(),
        })
    }
}

pub fn parse_chromium(bytes: &[u8]) -> Result<ImportedBookmarks, ImportError> {
    check_file_size(bytes.len() as u64)?;
    if !json_depth_ok(bytes) {
        return Err(ImportError::Malformed);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    deserializer.disable_recursion_limit();
    let value = JsonValue::deserialize(&mut deserializer).map_err(|_| ImportError::Malformed)?;
    deserializer.end().map_err(|_| ImportError::Malformed)?;
    let roots = value.get("roots").ok_or(ImportError::Malformed)?;
    let mut budget = Budget::default();
    let mut folders = Vec::new();
    for (key, fallback) in [
        ("bookmark_bar", "Bookmarks bar"),
        ("other", "Other bookmarks"),
        ("synced", "Mobile bookmarks"),
    ] {
        let Some(root) = roots.get(key) else { continue };
        let name = root
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or(fallback);
        if !budget.claim() {
            break;
        }
        let mut folder = ImportedFolder {
            name: sanitize_title(name),
            items: Vec::new(),
        };
        if let Some(children) = root.get("children").and_then(JsonValue::as_array) {
            chromium_children(children, &mut folder, 1, &mut budget);
        }
        folders.push(folder);
    }
    Ok(ImportedBookmarks {
        folders,
        skipped_invalid: budget.skipped_invalid,
    })
}

fn json_depth_ok(bytes: &[u8]) -> bool {
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;
    for &byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 256 {
                        return false;
                    }
                }
                b'}' | b']' => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    depth == 0 && !quoted
}

fn chromium_children(
    children: &[JsonValue],
    folder: &mut ImportedFolder,
    depth: usize,
    budget: &mut Budget,
) {
    for child in children {
        if budget.entries == MAX_ENTRIES {
            break;
        }
        match child.get("type").and_then(JsonValue::as_str) {
            Some("url") => {
                if let Some(url) = child.get("url").and_then(JsonValue::as_str) {
                    let title = child.get("name").and_then(JsonValue::as_str).unwrap_or("");
                    if let Some(link) = budget.link(title, url) {
                        folder.items.push(link);
                    }
                } else {
                    budget.skipped_invalid += 1;
                }
            }
            Some("folder") if depth < MAX_DEPTH => {
                if !budget.claim() {
                    break;
                }
                let name = child.get("name").and_then(JsonValue::as_str).unwrap_or("");
                let mut nested = ImportedFolder {
                    name: sanitize_title(name),
                    items: Vec::new(),
                };
                if let Some(children) = child.get("children").and_then(JsonValue::as_array) {
                    chromium_children(children, &mut nested, depth + 1, budget);
                }
                folder.items.push(ImportedItem::Folder(nested));
            }
            _ => {}
        }
    }
}

pub fn parse_safari(bytes: &[u8]) -> Result<ImportedBookmarks, ImportError> {
    check_file_size(bytes.len() as u64)?;
    let value = PlistValue::from_reader(Cursor::new(bytes)).map_err(|_| ImportError::Malformed)?;
    let children = plist_get(&value, "Children")
        .and_then(PlistValue::as_array)
        .ok_or(ImportError::Malformed)?;
    let mut budget = Budget::default();
    let mut folders = Vec::new();
    for child in children {
        if budget.entries == MAX_ENTRIES {
            break;
        }
        if safari_reading_list(child) {
            continue;
        }
        if let Some(folder) = safari_folder(child, 1, &mut budget) {
            folders.push(folder);
        }
    }
    Ok(ImportedBookmarks {
        folders,
        skipped_invalid: budget.skipped_invalid,
    })
}

fn safari_reading_list(node: &PlistValue) -> bool {
    let title = plist_get(node, "Title").and_then(PlistValue::as_string);
    let identifier = plist_get(node, "WebBookmarkIdentifier").and_then(PlistValue::as_string);
    matches!(title, Some("com.apple.ReadingList" | "Reading List"))
        || identifier == Some("com.apple.ReadingList")
}

fn safari_folder(node: &PlistValue, depth: usize, budget: &mut Budget) -> Option<ImportedFolder> {
    if depth > MAX_DEPTH
        || plist_get(node, "WebBookmarkType").and_then(PlistValue::as_string)
            != Some("WebBookmarkTypeList")
        || !budget.claim()
    {
        return None;
    }
    let name = plist_get(node, "Title")
        .and_then(PlistValue::as_string)
        .unwrap_or("");
    let mut folder = ImportedFolder {
        name: sanitize_title(name),
        items: Vec::new(),
    };
    if let Some(children) = plist_get(node, "Children").and_then(PlistValue::as_array) {
        for child in children {
            if budget.entries == MAX_ENTRIES {
                break;
            }
            if safari_reading_list(child) {
                continue;
            }
            match plist_get(child, "WebBookmarkType").and_then(PlistValue::as_string) {
                Some("WebBookmarkTypeList") => {
                    if let Some(nested) = safari_folder(child, depth + 1, budget) {
                        folder.items.push(ImportedItem::Folder(nested));
                    }
                }
                Some("WebBookmarkTypeLeaf") => {
                    let title = plist_get(child, "URIDictionary")
                        .and_then(|v| plist_get(v, "title"))
                        .and_then(PlistValue::as_string)
                        .or_else(|| plist_get(child, "Title").and_then(PlistValue::as_string))
                        .unwrap_or("");
                    if let Some(url) = plist_get(child, "URLString").and_then(PlistValue::as_string)
                    {
                        if let Some(link) = budget.link(title, url) {
                            folder.items.push(link);
                        }
                    } else {
                        budget.skipped_invalid += 1;
                    }
                }
                _ => {}
            }
        }
    }
    Some(folder)
}

fn plist_get<'a>(value: &'a PlistValue, key: &str) -> Option<&'a PlistValue> {
    value.as_dictionary()?.get(key)
}

pub fn parse_netscape_html(bytes: &[u8]) -> Result<ImportedBookmarks, ImportError> {
    check_file_size(bytes.len() as u64)?;
    let html = std::str::from_utf8(bytes).map_err(|_| ImportError::Malformed)?;
    let mut budget = Budget::default();
    let mut folders = Vec::new();
    let mut stack: Vec<Option<ImportedFolder>> = Vec::new();
    let mut pending: Option<String> = None;
    let mut pos = 0;
    while let Some(start) = html[pos..].find('<').map(|offset| pos + offset) {
        let Some(end) = tag_end(html, start + 1) else {
            break;
        };
        let tag = html[start + 1..end].trim();
        pos = end + 1;
        let name_end = tag
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let name = &tag[..name_end];
        if let Some(closing_tag) = tag.strip_prefix('/') {
            if closing_tag.trim().eq_ignore_ascii_case("dl")
                && let Some(finished) = stack.pop().flatten()
            {
                add_folder(&mut stack, &mut folders, finished);
            }
            continue;
        }
        if name.eq_ignore_ascii_case("h3") {
            if let Some((content, next)) = element_text(html, pos, "h3") {
                pending = Some(sanitize_title(&decode_entities(content)));
                pos = next;
            }
        } else if name.eq_ignore_ascii_case("dl") {
            if let Some(title) = pending.take() {
                if stack.len() < MAX_DEPTH && budget.claim() {
                    stack.push(Some(ImportedFolder {
                        name: title,
                        items: Vec::new(),
                    }));
                } else {
                    stack.push(None);
                }
            }
        } else if name.eq_ignore_ascii_case("a")
            && let Some((content, next)) = element_text(html, pos, "a")
        {
            pos = next;
            if stack.last().is_some_and(Option::is_none) {
                continue;
            }
            if let Some(raw_url) = href(tag) {
                let url = decode_entities(raw_url);
                if let Some(link) = budget.link(&decode_entities(content), &url) {
                    if let Some(Some(parent)) = stack.last_mut() {
                        parent.items.push(link);
                    } else {
                        add_root_link(&mut folders, link, &mut budget);
                    }
                }
            } else {
                budget.skipped_invalid += 1;
            }
        }
    }
    while let Some(node) = stack.pop() {
        if let Some(finished) = node {
            add_folder(&mut stack, &mut folders, finished);
        }
    }
    Ok(ImportedBookmarks {
        folders,
        skipped_invalid: budget.skipped_invalid,
    })
}

fn add_folder(
    stack: &mut [Option<ImportedFolder>],
    roots: &mut Vec<ImportedFolder>,
    folder: ImportedFolder,
) {
    if let Some(Some(parent)) = stack.last_mut() {
        parent.items.push(ImportedItem::Folder(folder));
    } else if stack.is_empty() {
        roots.push(folder);
    }
}

fn add_root_link(roots: &mut Vec<ImportedFolder>, link: ImportedItem, budget: &mut Budget) {
    if let Some(root) = roots.iter_mut().find(|root| root.name == "Bookmarks") {
        root.items.push(link);
    } else if budget.claim() {
        roots.push(ImportedFolder {
            name: "Bookmarks".to_owned(),
            items: vec![link],
        });
    }
}

fn tag_end(html: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, ch) in html[from..].char_indices() {
        if Some(ch) == quote {
            quote = None;
        } else if quote.is_none() && (ch == '"' || ch == '\'') {
            quote = Some(ch);
        } else if quote.is_none() && ch == '>' {
            return Some(from + offset);
        }
    }
    None
}

fn element_text<'a>(html: &'a str, from: usize, name: &str) -> Option<(&'a str, usize)> {
    let closing = format!("</{name}");
    let offset = html.as_bytes()[from..]
        .windows(closing.len())
        .position(|window| window.eq_ignore_ascii_case(closing.as_bytes()))?;
    let close = from + offset;
    let end = tag_end(html, close + 1)?;
    Some((&html[from..close], end + 1))
}

fn href(tag: &str) -> Option<&str> {
    let bytes = tag.as_bytes();
    let mut pos = 0;
    while pos + 4 <= bytes.len() {
        if (pos == 0 || bytes[pos - 1].is_ascii_whitespace())
            && tag[pos..]
                .get(..4)
                .is_some_and(|part| part.eq_ignore_ascii_case("href"))
        {
            let mut value = pos + 4;
            while bytes.get(value).is_some_and(u8::is_ascii_whitespace) {
                value += 1;
            }
            if bytes.get(value) != Some(&b'=') {
                pos += 1;
                continue;
            }
            value += 1;
            while bytes.get(value).is_some_and(u8::is_ascii_whitespace) {
                value += 1;
            }
            let quote = bytes.get(value).copied();
            if quote == Some(b'"') || quote == Some(b'\'') {
                let start = value + 1;
                let end = tag[start..].find(quote? as char)? + start;
                return tag.get(start..end);
            }
            let end = tag[value..]
                .find(char::is_whitespace)
                .map_or(tag.len(), |n| value + n);
            return tag.get(value..end);
        }
        pos += 1;
        while pos < bytes.len() && !tag.is_char_boundary(pos) {
            pos += 1;
        }
    }
    None
}

fn decode_entities(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(end) = rest.find(';') else { break };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|value| u32::from_str_radix(value, 16).ok())
                .and_then(char::from_u32)
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .and_then(|value| value.parse::<u32>().ok())
                        .and_then(char::from_u32)
                }),
        };
        if let Some(ch) = decoded {
            out.push(ch);
        } else {
            out.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn to_tab_plan(bookmarks: &ImportedBookmarks) -> TabPlan {
    let mut groups = Vec::new();
    for folder in &bookmarks.folders {
        let mut tabs = Vec::new();
        flatten(&folder.items, "", &mut tabs);
        groups.push(PlannedGroup {
            name: folder.name.clone(),
            tabs,
        });
    }
    TabPlan { groups }
}

fn flatten(items: &[ImportedItem], prefix: &str, tabs: &mut Vec<PlannedTab>) {
    for item in items {
        match item {
            ImportedItem::Link { title, url } => {
                let title = if prefix.is_empty() {
                    title.clone()
                } else {
                    format!("{prefix} › {title}")
                };
                tabs.push(PlannedTab {
                    title,
                    url: url.clone(),
                    loaded: false,
                });
            }
            ImportedItem::Folder(folder) => {
                let prefix = if prefix.is_empty() {
                    folder.name.clone()
                } else {
                    format!("{prefix} › {}", folder.name)
                };
                flatten(&folder.items, &prefix, tabs);
            }
        }
    }
}
