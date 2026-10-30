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
    /// whether a corrupt main file was moved aside
    pub main_quarantined: bool,
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
pub fn load(path: &Path) -> Result<Loaded, SessionError> {
    let backup = suffixed(path, ".bak");
    match load_one(path) {
        Ok(browser) => Ok(Loaded {
            browser,
            source: Source::Main,
            main_quarantined: false,
        }),
        Err(SessionError::UnsupportedVersion(version)) => {
            Err(SessionError::UnsupportedVersion(version))
        }
        Err(main_error) => {
            if !backup.exists() {
                if matches!(main_error, SessionError::Missing) {
                    return Err(SessionError::Missing);
                }
                return Err(main_error);
            }
            let browser = load_one(&backup)?;
            let main_quarantined = if matches!(
                main_error,
                SessionError::Parse(_) | SessionError::Corrupt(_)
            ) {
                quarantine_main(path).is_ok()
            } else {
                false
            };
            Ok(Loaded {
                browser,
                source: Source::Backup,
                main_quarantined,
            })
        }
    }
}

fn quarantine_main(path: &Path) -> Result<(), std::io::Error> {
    let corrupt = suffixed(path, ".corrupt");
    if corrupt.exists() {
        fs::remove_file(&corrupt)?;
    }
    fs::rename(path, corrupt)
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
    fn corrupt_main_falls_back_to_backup() {
        let dir = test_dir();
        let path = dir.join("session.json");
        save(&path, &Browser::new()).unwrap();
        save(&path, &Browser::new()).unwrap();
        fs::write(&path, b"broken").unwrap();
        fs::write(suffixed(&path, ".corrupt"), b"older").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.source, Source::Backup);
        assert!(loaded.main_quarantined);
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
                },
                1,
            )
            .unwrap();
        save(&path, &browser).unwrap();
        save(&path, &browser).unwrap();
        fs::write(&path, b"broken").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.source, Source::Backup);
        assert!(loaded.main_quarantined);
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
        assert!(!loaded.main_quarantined);
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
    fn newer_version_is_rejected() {
        let dir = test_dir();
        let path = dir.join("session.json");
        fs::write(&path, b"{\"version\":2,\"browser\":{}}").unwrap();
        assert!(matches!(
            load(&path),
            Err(SessionError::UnsupportedVersion(2))
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_session_is_distinct() {
        let dir = test_dir();
        assert!(matches!(
            load(&dir.join("missing.json")),
            Err(SessionError::Missing)
        ));
        fs::remove_dir_all(dir).unwrap();
    }
}
