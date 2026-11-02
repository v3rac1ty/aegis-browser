// test code may panic on bad fixtures
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aegis_import::{
    ImportError, ImportedItem, MAX_DEPTH, MAX_FILE_BYTES, Platform, SkipReason, Source,
    check_file_size, discover, parse_chromium, parse_netscape_html, parse_password_csv,
    parse_safari, to_tab_plan,
};

struct FakeHome(PathBuf);

impl FakeHome {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("aegis-import-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn touch(&self, relative: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"{}").unwrap();
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FakeHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn chromium_nested_invalid_and_sanitized() {
    let imported = parse_chromium(include_bytes!("fixtures/chromium.json")).unwrap();
    assert_eq!(imported.skipped_invalid, 2);
    assert_eq!(imported.folders.len(), 3);
    assert_eq!(imported.folders[0].name, "Bar title");
    let plan = to_tab_plan(&imported);
    assert_eq!(plan.groups[0].tabs.len(), 2);
    assert_eq!(plan.groups[0].tabs[0].title, "A B");
    assert_eq!(plan.groups[0].tabs[1].title, "Work › Nested");
    assert!(!plan.groups[0].tabs[1].loaded);
}

#[test]
fn safari_nested_invalid_and_reading_list() {
    let imported = parse_safari(include_bytes!("fixtures/safari.plist")).unwrap();
    assert_eq!(imported.skipped_invalid, 1);
    assert_eq!(imported.folders.len(), 1);
    assert!(
        matches!(&imported.folders[0].items[0], ImportedItem::Link { title, .. } if title == "Safe title")
    );
    assert_eq!(
        to_tab_plan(&imported).groups[0].tabs[1].title,
        "Nested › Child"
    );
}

#[test]
fn netscape_nested_entities_and_invalid() {
    let imported = parse_netscape_html(include_bytes!("fixtures/bookmarks.html")).unwrap();
    assert_eq!(imported.skipped_invalid, 1);
    assert_eq!(imported.folders[0].name, "Bar title");
    let tabs = &to_tab_plan(&imported).groups[0].tabs;
    assert_eq!(tabs[0].title, "Good & Safe");
    assert_eq!(tabs[0].url, "https://example.com/?a=1&b=2");
    assert_eq!(tabs[1].title, "Nested › Child");
}

#[test]
fn chromium_depth_cap() {
    let mut node =
        String::from("{\"type\":\"url\",\"name\":\"Too deep\",\"url\":\"https://example.com/\"}");
    for _ in 0..MAX_DEPTH {
        node = format!("{{\"type\":\"folder\",\"name\":\"Layer\",\"children\":[{node}]}}");
    }
    let data = format!(
        "{{\"roots\":{{\"bookmark_bar\":{{\"type\":\"folder\",\"name\":\"Bar\",\"children\":[{node}]}}}}}}"
    );
    let imported = parse_chromium(data.as_bytes()).unwrap();
    assert!(to_tab_plan(&imported).groups[0].tabs.is_empty());
}

#[test]
fn netscape_depth_cap() {
    let mut html = String::from("<DL>");
    for _ in 0..=MAX_DEPTH {
        html.push_str("<DT><H3>Layer</H3><DL>");
    }
    html.push_str("<DT><A HREF=\"https://example.com/\">Deep</A>");
    for _ in 0..=MAX_DEPTH {
        html.push_str("</DL>");
    }
    html.push_str("</DL>");
    let imported = parse_netscape_html(html.as_bytes()).unwrap();
    assert!(to_tab_plan(&imported).groups[0].tabs.is_empty());
}

#[test]
fn safari_depth_cap() {
    let mut xml = String::from(
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>Children</key><array>",
    );
    for _ in 0..=MAX_DEPTH {
        xml.push_str("<dict><key>WebBookmarkType</key><string>WebBookmarkTypeList</string><key>Title</key><string>Layer</string><key>Children</key><array>");
    }
    xml.push_str("<dict><key>WebBookmarkType</key><string>WebBookmarkTypeLeaf</string><key>URLString</key><string>https://example.com/</string></dict>");
    for _ in 0..=MAX_DEPTH {
        xml.push_str("</array></dict>");
    }
    xml.push_str("</array></dict></plist>");
    let imported = parse_safari(xml.as_bytes()).unwrap();
    assert!(to_tab_plan(&imported).groups[0].tabs.is_empty());
}

#[test]
fn discover_windows_profiles() {
    let home = FakeHome::new();
    home.touch("AppData/Local/Google/Chrome/User Data/Default/Bookmarks");
    home.touch("AppData/Local/Google/Chrome/User Data/Profile 2/Bookmarks");
    home.touch("AppData/Local/Microsoft/Edge/User Data/Default/Bookmarks");
    home.touch("AppData/Roaming/Opera Software/Opera Stable/Bookmarks");
    let found = discover(home.path(), Platform::Windows);
    assert_eq!(found.len(), 4);
    assert_eq!(found[0].source, Source::Chrome);
    assert_eq!(found[1].name, "Profile 2");
    assert_eq!(found[3].source, Source::Opera);
}

#[test]
fn discover_macos_profiles_and_safari() {
    let home = FakeHome::new();
    home.touch("Library/Application Support/Arc/User Data/Default/Bookmarks");
    home.touch("Library/Application Support/BraveSoftware/Brave-Browser/Profile 1/Bookmarks");
    home.touch("Library/Safari/Bookmarks.plist");
    let found = discover(home.path(), Platform::Macos);
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].source, Source::Brave);
    assert_eq!(found[1].source, Source::Arc);
    assert_eq!(found[2].source, Source::Safari);
}

#[test]
fn discover_linux_profiles() {
    let home = FakeHome::new();
    home.touch(".config/vivaldi/Default/Bookmarks");
    home.touch(".config/opera/Bookmarks");
    let found = discover(home.path(), Platform::Linux);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].source, Source::Vivaldi);
    assert_eq!(found[1].source, Source::Opera);
}

#[test]
fn chrome_csv_skips_http_and_redacts_debug() {
    let imported = parse_password_csv(include_bytes!("fixtures/chrome.csv")).unwrap();
    assert_eq!(imported.logins.len(), 1);
    assert_eq!(imported.logins[0].origin, "https://example.com");
    assert_eq!(imported.skipped, [SkipReason::NonHttps]);
    let debug = format!("{:?}", imported.logins[0]);
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("secret"));
}

#[test]
fn firefox_csv_quoted_fields() {
    let imported = parse_password_csv(include_bytes!("fixtures/firefox.csv")).unwrap();
    assert_eq!(imported.logins[0].origin, "https://example.org:8443");
    assert_eq!(imported.logins[0].username, "a,b");
    assert_eq!(*imported.logins[0].password, "say \"hi\"");
}

#[test]
fn safari_csv_case_insensitive_headers() {
    let imported = parse_password_csv(include_bytes!("fixtures/safari.csv")).unwrap();
    assert_eq!(imported.logins[0].origin, "https://example.net");
    assert_eq!(imported.logins[0].username, "carol");
}

#[test]
fn csv_missing_column_errors() {
    assert_eq!(
        parse_password_csv(b"url,username\nhttps://example.com,a\n").unwrap_err(),
        ImportError::MissingColumns
    );
}

#[test]
fn fake_length_over_50_mb_is_refused() {
    assert_eq!(check_file_size(MAX_FILE_BYTES), Ok(()));
    assert_eq!(
        check_file_size(MAX_FILE_BYTES + 1),
        Err(ImportError::TooLarge)
    );
}
