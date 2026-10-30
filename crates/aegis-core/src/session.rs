use std::fmt;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Browser, InvariantError};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    browser: Browser,
}

#[derive(Serialize)]
struct SessionFileRef<'a> {
    version: u32,
    browser: &'a Browser,
}

#[derive(Deserialize)]
struct VersionHeader {
    version: u32,
}

/// which file supplied a valid session
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// the requested session file
    Main,
    /// the backup session file
    Backup,
}

/// a restored browser and its source
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    /// restored browser state
    pub browser: Browser,
    /// file used to restore it
    pub source: Source,
    /// where the unusable main file was moved
    pub main_moved_to: Option<PathBuf>,
    /// whether moving the unusable main file failed
    pub main_move_failed: bool,
}

/// a failed restore and the fate of the main session file
#[derive(Debug)]
pub struct LoadFailure {
    /// why the session could not be restored
    pub error: SessionError,
    /// where the unusable main file was moved
    pub moved_to: Option<PathBuf>,
    /// whether moving the unusable main file failed
    pub move_failed: bool,
}

/// session read, write, or validation failure
#[derive(Debug)]
pub enum SessionError {
    /// a file operation failed
    Io(std::io::Error),
    /// the JSON could not be parsed
    Parse(serde_json::Error),
    /// the file uses a newer schema
    UnsupportedVersion(u32),
    /// the browser state breaks an invariant
    Corrupt(InvariantError),
    /// no main session or backup exists
    Missing,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "session I/O: {error}"),
            Self::Parse(error) => write!(f, "session JSON: {error}"),
            Self::UnsupportedVersion(version) => write!(f, "unsupported session version {version}"),
            Self::Corrupt(error) => write!(f, "corrupt session: {error}"),
            Self::Missing => write!(f, "no session file"),
        }
    }
}

impl std::error::Error for SessionError {}

/// writes a versioned session through a synced temporary file
pub fn save(path: &Path, browser: &Browser) -> Result<(), SessionError> {
    let tmp = suffixed(path, ".tmp");
    let backup = suffixed(path, ".bak");
    let file = File::create(&tmp).map_err(SessionError::Io)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(
        &mut writer,
        &SessionFileRef {
            version: VERSION,
            browser,
        },
    )
    .map_err(SessionError::Parse)?;
    writer.flush().map_err(SessionError::Io)?;
    let file = writer
        .into_inner()
        .map_err(|error| SessionError::Io(error.into_error()))?;
    file.sync_all().map_err(SessionError::Io)?;
    drop(file);
    // keep the backup when the main session cannot be restored
    if load_one(path).is_ok() {
        fs::copy(path, backup).map_err(SessionError::Io)?;
    }
    fs::rename(tmp, path).map_err(SessionError::Io)?;
    Ok(())
}

/// restores the main session or its backup after corruption
pub fn load(path: &Path) -> Result<Loaded, LoadFailure> {
    let backup = suffixed(path, ".bak");
    match load_one(path) {
        Ok(browser) => Ok(Loaded {
            browser,
            source: Source::Main,
            main_moved_to: None,
            main_move_failed: false,
        }),
        Err(main_error) => {
            let (moved_to, move_failed) = move_unusable_main(path, &main_error);
            if matches!(main_error, SessionError::UnsupportedVersion(_)) || !backup.exists() {
                return Err(LoadFailure {
                    error: main_error,
                    moved_to,
                    move_failed,
                });
            }
            let browser = load_one(&backup).map_err(|error| LoadFailure {
                error,
                moved_to: moved_to.clone(),
                move_failed,
            })?;
            Ok(Loaded {
                browser,
                source: Source::Backup,
                main_moved_to: moved_to,
                main_move_failed: move_failed,
            })
        }
    }
}

fn move_unusable_main(path: &Path, error: &SessionError) -> (Option<PathBuf>, bool) {
    let target = match error {
        SessionError::Missing => return (None, false),
        SessionError::UnsupportedVersion(version) => suffixed(path, &format!(".v{version}")),
        _ => suffixed(path, ".corrupt"),
    };
    if target.exists() && fs::remove_file(&target).is_err() {
        return (None, true);
    }
    match fs::rename(path, &target) {
        Ok(()) => (Some(target), false),
        Err(_) => (None, true),
    }
}

fn load_one(path: &Path) -> Result<Browser, SessionError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(SessionError::Missing);
        }
        Err(error) => return Err(SessionError::Io(error)),
    };
    let header: VersionHeader = serde_json::from_slice(&bytes).map_err(SessionError::Parse)?;
    if header.version != VERSION {
        return Err(SessionError::UnsupportedVersion(header.version));
    }
    let session: SessionFile = serde_json::from_slice(&bytes).map_err(SessionError::Parse)?;
    session
        .browser
        .check_invariants()
        .map_err(SessionError::Corrupt)?;
    Ok(session.browser)
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{Command, Placement};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn test_dir() -> PathBuf {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("aegis-core-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn round_trip_restores_unloaded_tabs() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let mut browser = Browser::new();
        browser
            .apply(
                Command::OpenTab {
                    url: "https://example.com".into(),
                    placement: Placement::End,
                    activate: true,
                    opener: None,
                },
                4,
            )
            .unwrap();
        save(&path, &browser).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.source, Source::Main);
        let id = loaded.browser.active_tab().unwrap();
        assert_eq!(
            loaded.browser.tab(id).unwrap().load(),
            crate::LoadState::Unloaded
        );
        assert_eq!(loaded.browser.check_invariants(), Ok(()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn opener_survives_round_trip_and_old_sessions_default_to_none() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let mut browser = Browser::new();
        browser
            .apply(
                Command::OpenTab {
                    url: "https://source.test".into(),
                    placement: Placement::End,
                    activate: true,
                    opener: None,
                },
                1,
            )
            .unwrap();
        let opener = browser.active_tab().unwrap();
        browser
            .apply(
                Command::OpenTab {
                    url: "https://popup.test".into(),
                    placement: Placement::End,
                    activate: true,
                    opener: Some(opener),
                },
                2,
            )
            .unwrap();
        let popup = browser.active_tab().unwrap();
        save(&path, &browser).unwrap();
        assert_eq!(
            load(&path).unwrap().browser.tab(popup).unwrap().opener(),
            Some(opener)
        );

        let mut old_session: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let tabs = old_session["browser"]["tabs"].as_object_mut().unwrap();
        for tab in tabs.values_mut() {
            tab.as_object_mut().unwrap().remove("opener");
        }
        fs::write(&path, serde_json::to_vec(&old_session).unwrap()).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.browser.tab(popup).unwrap().opener(), None);
        assert_eq!(loaded.browser.check_invariants(), Ok(()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_main_falls_back_to_backup() {
        let dir = test_dir();
        let path = dir.join("session.json");
        save(&path, &Browser::new()).unwrap();
        save(&path, &Browser::new()).unwrap();
        fs::write(&path, b"broken").unwrap();
        fs::write(suffixed(&path, ".corrupt"), b"older").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.source, Source::Backup);
        assert_eq!(loaded.main_moved_to, Some(suffixed(&path, ".corrupt")));
        assert!(!loaded.main_move_failed);
        assert_eq!(fs::read(suffixed(&path, ".corrupt")).unwrap(), b"broken");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_main_then_save_preserves_valid_backup() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let mut browser = Browser::new();
        browser
            .apply(
                Command::OpenTab {
                    url: "saved".into(),
                    placement: Placement::End,
                    activate: false,
                    opener: None,
                },
                1,
            )
            .unwrap();
        save(&path, &browser).unwrap();
        save(&path, &browser).unwrap();
        fs::write(&path, b"broken").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.source, Source::Backup);
        assert_eq!(loaded.main_moved_to, Some(suffixed(&path, ".corrupt")));
        assert!(!loaded.main_move_failed);
        save(&path, &loaded.browser).unwrap();
        assert_eq!(load_one(&suffixed(&path, ".bak")).unwrap(), browser);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_preserves_backup_when_corrupt_main_remains() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let backup = suffixed(&path, ".bak");
        let original = Browser::new();
        let mut replacement = Browser::new();
        replacement
            .apply(
                Command::OpenTab {
                    url: "new".into(),
                    placement: Placement::End,
                    activate: false,
                    opener: None,
                },
                1,
            )
            .unwrap();

        save(&path, &original).unwrap();
        fs::copy(&path, &backup).unwrap();
        fs::write(&path, b"broken").unwrap();
        save(&path, &replacement).unwrap();

        assert_eq!(load_one(&backup).unwrap(), original);
        assert_eq!(load_one(&path).unwrap(), replacement);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_quarantine_is_reported_with_backup() {
        let dir = test_dir();
        let path = dir.join("session.json");
        save(&path, &Browser::new()).unwrap();
        save(&path, &Browser::new()).unwrap();
        fs::write(&path, b"broken").unwrap();
        fs::create_dir(suffixed(&path, ".corrupt")).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.source, Source::Backup);
        assert_eq!(loaded.main_moved_to, None);
        assert!(loaded.main_move_failed);
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_main_state_falls_back_to_backup() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let mut browser = Browser::new();
        save(&path, &browser).unwrap();
        browser.next_id = 1;
        save(&path, &browser).unwrap();
        assert_eq!(load(&path).unwrap().source, Source::Backup);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_main_without_backup_is_kept_after_save() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let corrupt = suffixed(&path, ".corrupt");
        fs::write(&path, b"broken").unwrap();
        let failure = load(&path).unwrap_err();
        assert!(matches!(failure.error, SessionError::Parse(_)));
        assert_eq!(failure.moved_to, Some(corrupt.clone()));
        assert!(!failure.move_failed);
        assert!(!path.exists());
        save(&path, &Browser::new()).unwrap();
        assert_eq!(fs::read(&corrupt).unwrap(), b"broken");
        assert!(load(&path).is_ok());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_move_without_backup_is_reported() {
        let dir = test_dir();
        let path = dir.join("session.json");
        fs::write(&path, b"broken").unwrap();
        fs::create_dir(suffixed(&path, ".corrupt")).unwrap();
        let failure = load(&path).unwrap_err();
        assert!(matches!(failure.error, SessionError::Parse(_)));
        assert_eq!(failure.moved_to, None);
        assert!(failure.move_failed);
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_version_without_backup_is_moved() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let newer = b"{\"version\":2,\"browser\":{}}";
        fs::write(&path, newer).unwrap();
        let failure = load(&path).unwrap_err();
        assert!(matches!(failure.error, SessionError::UnsupportedVersion(2)));
        assert_eq!(failure.moved_to, Some(suffixed(&path, ".v2")));
        assert_eq!(fs::read(suffixed(&path, ".v2")).unwrap(), newer);
        assert!(!path.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newer_version_with_backup_is_moved_without_using_backup() {
        let dir = test_dir();
        let path = dir.join("session.json");
        let backup = suffixed(&path, ".bak");
        save(&path, &Browser::new()).unwrap();
        fs::copy(&path, &backup).unwrap();
        let backup_bytes = fs::read(&backup).unwrap();
        fs::write(&path, b"{\"version\":2,\"browser\":{}}").unwrap();
        let failure = load(&path).unwrap_err();
        assert!(matches!(failure.error, SessionError::UnsupportedVersion(2)));
        assert_eq!(failure.moved_to, Some(suffixed(&path, ".v2")));
        assert_eq!(fs::read(backup).unwrap(), backup_bytes);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_session_is_distinct() {
        let dir = test_dir();
        assert!(matches!(
            load(&dir.join("missing.json")),
            Err(LoadFailure {
                error: SessionError::Missing,
                ..
            })
        ));
        fs::remove_dir_all(dir).unwrap();
    }
}
