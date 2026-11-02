use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Chrome,
    Edge,
    Brave,
    Vivaldi,
    Arc,
    Opera,
    Safari,
    Firefox,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Windows,
    Macos,
    Linux,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoundProfile {
    pub source: Source,
    pub name: String,
    pub bookmarks_path: PathBuf,
}

// known paths relative to home; firefox uses a user-selected html export
// | source   | windows                                      | macos                                                   | linux                               |
// | chrome   | AppData/Local/Google/Chrome/User Data         | Library/Application Support/Google/Chrome               | .config/google-chrome               |
// | edge     | AppData/Local/Microsoft/Edge/User Data        | Library/Application Support/Microsoft Edge              | .config/microsoft-edge              |
// | brave    | AppData/Local/BraveSoftware/Brave-Browser/User Data | Library/Application Support/BraveSoftware/Brave-Browser | .config/BraveSoftware/Brave-Browser |
// | vivaldi  | AppData/Local/Vivaldi/User Data               | Library/Application Support/Vivaldi                     | .config/vivaldi                     |
// | arc      | AppData/Local/Packages/TheBrowserCompany.Arc_ttt1ap7aakyb4/LocalCache/Local/Arc/User Data | Library/Application Support/Arc/User Data | none |
// | opera    | AppData/Roaming/Opera Software/Opera Stable   | Library/Application Support/com.operasoftware.Opera     | .config/opera                       |
// | safari   | none                                         | Library/Safari/Bookmarks.plist                          | none                                |
const PROFILE_PATHS: &[(Source, Platform, &str)] = &[
    (
        Source::Chrome,
        Platform::Windows,
        "AppData/Local/Google/Chrome/User Data",
    ),
    (
        Source::Chrome,
        Platform::Macos,
        "Library/Application Support/Google/Chrome",
    ),
    (Source::Chrome, Platform::Linux, ".config/google-chrome"),
    (
        Source::Edge,
        Platform::Windows,
        "AppData/Local/Microsoft/Edge/User Data",
    ),
    (
        Source::Edge,
        Platform::Macos,
        "Library/Application Support/Microsoft Edge",
    ),
    (Source::Edge, Platform::Linux, ".config/microsoft-edge"),
    (
        Source::Brave,
        Platform::Windows,
        "AppData/Local/BraveSoftware/Brave-Browser/User Data",
    ),
    (
        Source::Brave,
        Platform::Macos,
        "Library/Application Support/BraveSoftware/Brave-Browser",
    ),
    (
        Source::Brave,
        Platform::Linux,
        ".config/BraveSoftware/Brave-Browser",
    ),
    (
        Source::Vivaldi,
        Platform::Windows,
        "AppData/Local/Vivaldi/User Data",
    ),
    (
        Source::Vivaldi,
        Platform::Macos,
        "Library/Application Support/Vivaldi",
    ),
    (Source::Vivaldi, Platform::Linux, ".config/vivaldi"),
    (
        Source::Arc,
        Platform::Windows,
        "AppData/Local/Packages/TheBrowserCompany.Arc_ttt1ap7aakyb4/LocalCache/Local/Arc/User Data",
    ),
    (
        Source::Arc,
        Platform::Macos,
        "Library/Application Support/Arc/User Data",
    ),
    (
        Source::Opera,
        Platform::Windows,
        "AppData/Roaming/Opera Software/Opera Stable",
    ),
    (
        Source::Opera,
        Platform::Macos,
        "Library/Application Support/com.operasoftware.Opera",
    ),
    (Source::Opera, Platform::Linux, ".config/opera"),
];

pub fn discover(home: &Path, platform: Platform) -> Vec<FoundProfile> {
    let mut found = Vec::new();
    for &(source, os, relative) in PROFILE_PATHS {
        if os != platform {
            continue;
        }
        let base = home.join(relative);
        add_profile(
            &mut found,
            source,
            "Default",
            base.join("Default/Bookmarks"),
        );
        if source == Source::Opera {
            add_profile(&mut found, source, "Default", base.join("Bookmarks"));
        }
        let Ok(entries) = fs::read_dir(&base) else {
            continue;
        };
        let mut profiles = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(number) = name.strip_prefix("Profile ")
                && number.parse::<u32>().is_ok()
            {
                profiles.push((name, entry.path().join("Bookmarks")));
            }
        }
        profiles.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, path) in profiles {
            add_profile(&mut found, source, &name, path);
        }
    }
    if platform == Platform::Macos {
        add_profile(
            &mut found,
            Source::Safari,
            "Safari",
            home.join("Library/Safari/Bookmarks.plist"),
        );
    }
    found
}

fn add_profile(found: &mut Vec<FoundProfile>, source: Source, name: &str, path: PathBuf) {
    if path.is_file() {
        found.push(FoundProfile {
            source,
            name: name.to_owned(),
            bookmarks_path: path,
        });
    }
}
