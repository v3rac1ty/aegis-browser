use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use aegis_core::{Browser, Command, CommandError, GroupColor, Item, Placement, WorkspaceId};
use aegis_import::TabPlan;
use aegis_import::{FoundProfile, ImportedBookmarks, PasswordImport, Platform, SkipReason, Source};
use eframe::egui;
use zeroize::Zeroizing;

pub enum Action {
    Bookmarks(PathBuf, String),
    Passwords,
    Cancel,
    DeleteCsv,
}

struct CsvIdentity {
    len: u64,
    modified: SystemTime,
    fingerprint: u64,
}

pub enum DeleteCsvError {
    Changed,
    Io(std::io::Error),
}

pub struct Imports {
    profiles: Option<Vec<FoundProfile>>,
    pub preview: Option<PasswordImport>,
    pub csv_path: Option<PathBuf>,
    csv_identity: Option<CsvIdentity>,
    pub delete_path: Option<PathBuf>,
    delete_identity: Option<CsvIdentity>,
    pub message: Option<String>,
}

impl Imports {
    pub fn new() -> Self {
        Self {
            profiles: None,
            preview: None,
            csv_path: None,
            csv_identity: None,
            delete_path: None,
            delete_identity: None,
            message: None,
        }
    }

    fn profiles(&mut self) -> &[FoundProfile] {
        self.profiles.get_or_insert_with(|| {
            let home = std::env::var_os(if cfg!(target_os = "windows") {
                "USERPROFILE"
            } else {
                "HOME"
            })
            .map(PathBuf::from)
            .unwrap_or_default();
            let platform = if cfg!(target_os = "windows") {
                Platform::Windows
            } else if cfg!(target_os = "macos") {
                Platform::Macos
            } else {
                Platform::Linux
            };
            aegis_import::discover(&home, platform)
        })
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme: crate::theme::Theme) -> Vec<Action> {
        use crate::settings_page::ui as sui;

        let mut actions = Vec::new();
        let profiles = self.profiles().to_vec();
        sui::section(ui, theme, "BOOKMARKS", |ui| {
            let mut first = true;
            for profile in profiles {
                sui::divider(ui, theme, &mut first);
                sui::row(
                    ui,
                    theme,
                    &format!("{:?} · {}", profile.source, profile.name),
                    sui::BUTTON,
                    |ui| {
                        if sui::button(ui, "Import", theme, false).clicked() {
                            actions.push(Action::Bookmarks(
                                profile.bookmarks_path,
                                format!("{:?}", profile.source),
                            ));
                        }
                    },
                );
            }
            sui::divider(ui, theme, &mut first);
            sui::row_hint(
                ui,
                theme,
                "Bookmarks HTML",
                Some("Drop an export from Firefox, Safari, Chrome, or Edge"),
                sui::NONE,
                |_| {},
            );
        });
        sui::section(ui, theme, "PASSWORDS", |ui| {
            let mut first = true;
            for (label, hint) in [
                (
                    "Passwords CSV",
                    "Drop a CSV exported from your other browser",
                ),
                (
                    "Chrome or Edge",
                    "Settings > Password Manager > Export passwords",
                ),
                ("Firefox", "Passwords > menu > Export Passwords"),
                ("Safari", "File > Export > Passwords"),
            ] {
                sui::divider(ui, theme, &mut first);
                sui::row_hint(ui, theme, label, Some(hint), sui::NONE, |_| {});
            }
            if let Some(preview) = &self.preview {
                let (invalid, non_https) = preview_counts(preview);
                sui::divider(ui, theme, &mut first);
                let summary = format!(
                    "{} logins, {} skipped ({} invalid URL, {} not https)",
                    preview.logins.len(),
                    preview.skipped.len(),
                    invalid,
                    non_https
                );
                let pair = sui::Size { w: 224.0, h: 28.0 };
                sui::row(ui, theme, &summary, pair, |ui| {
                    if sui::button(ui, "Import passwords", theme, true).clicked() {
                        actions.push(Action::Passwords);
                    }
                    if sui::button(ui, "Cancel", theme, false).clicked() {
                        actions.push(Action::Cancel);
                    }
                });
            }
            if self.delete_path.is_some() {
                sui::divider(ui, theme, &mut first);
                sui::row(
                    ui,
                    theme,
                    "It contains your passwords in plain text.",
                    sui::BUTTON,
                    |ui| {
                        if sui::button(ui, "Delete the CSV file", theme, false).clicked() {
                            actions.push(Action::DeleteCsv);
                        }
                    },
                );
            }
        });
        sui::section(ui, theme, "HISTORY", |ui| {
            sui::row(
                ui,
                theme,
                "History import is coming later",
                sui::NONE,
                |_| {},
            );
            if let Some(message) = &self.message {
                sui::hairline(ui, theme);
                sui::row(ui, theme, message, sui::NONE, |_| {});
            }
        });
        let dropped = ui.ctx().input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter(|file| !file.path().as_os_str().is_empty())
                .map(|file| file.path().to_owned())
                .collect::<Vec<_>>()
        });
        for path in dropped {
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
            {
                self.load_csv(&path);
            } else if path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm")
            }) {
                actions.push(Action::Bookmarks(path, "HTML export".to_owned()));
            }
        }
        actions
    }

    fn load_csv(&mut self, path: &Path) {
        let result = read_limited(path)
            .map_err(|_| "Couldn't read the CSV file".to_owned())
            .and_then(|bytes| {
                let identity = csv_identity(path, &bytes)
                    .map_err(|_| "Couldn't read the CSV file".to_owned())?;
                let preview = aegis_import::parse_password_csv(&bytes)
                    .map_err(|_| "Couldn't parse the CSV file".to_owned())?;
                Ok((preview, identity))
            });
        match result {
            Ok((preview, identity)) => {
                self.preview = Some(preview);
                self.csv_path = Some(path.to_owned());
                self.csv_identity = Some(identity);
                self.message = None;
            }
            Err(message) => {
                self.preview = None;
                self.csv_path = None;
                self.csv_identity = None;
                self.message = Some(message);
            }
        }
    }

    pub fn clear_preview(&mut self) {
        self.preview = None;
        self.csv_path = None;
        self.csv_identity = None;
    }

    pub fn clear_passwords(&mut self) {
        self.clear_preview();
        self.delete_path = None;
        self.delete_identity = None;
        self.message = None;
    }

    pub fn mark_imported(&mut self) {
        self.delete_path = self.csv_path.take();
        self.delete_identity = self.csv_identity.take();
    }

    pub fn delete_csv(&mut self) -> Result<(), DeleteCsvError> {
        let Some(path) = self.delete_path.take() else {
            return Ok(());
        };
        let Some(expected) = self.delete_identity.take() else {
            return Err(DeleteCsvError::Changed);
        };
        let bytes = read_limited(&path).map_err(DeleteCsvError::Io)?;
        let current = csv_identity(&path, &bytes).map_err(DeleteCsvError::Io)?;
        if current.len != expected.len
            || current.modified != expected.modified
            || current.fingerprint != expected.fingerprint
        {
            return Err(DeleteCsvError::Changed);
        }
        std::fs::remove_file(path).map_err(DeleteCsvError::Io)?;
        Ok(())
    }
}

fn csv_identity(path: &Path, bytes: &[u8]) -> Result<CsvIdentity, std::io::Error> {
    let metadata = std::fs::metadata(path)?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(CsvIdentity {
        len: metadata.len(),
        modified: metadata.modified()?,
        fingerprint: hasher.finish(),
    })
}

pub fn parse_bookmarks(path: &Path, source: &str) -> Result<ImportedBookmarks, String> {
    let bytes = read_limited(path).map_err(|_| "Couldn't read bookmarks".to_owned())?;
    let parsed = if source == format!("{:?}", Source::Safari)
        || path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("plist"))
    {
        aegis_import::parse_safari(&bytes)
    } else if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
    {
        aegis_import::parse_netscape_html(&bytes)
    } else {
        aegis_import::parse_chromium(&bytes)
    };
    parsed.map_err(|_| "Couldn't parse bookmarks".to_owned())
}

fn read_limited(path: &Path) -> Result<Zeroizing<Vec<u8>>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(aegis_import::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > aegis_import::MAX_FILE_BYTES {
        return Err(std::io::Error::other("import file is too large"));
    }
    Ok(bytes)
}

pub fn preview_counts(preview: &PasswordImport) -> (usize, usize) {
    let mut invalid = 0;
    let mut non_https = 0;
    for reason in &preview.skipped {
        match reason {
            SkipReason::InvalidUrl => invalid += 1,
            SkipReason::NonHttps => non_https += 1,
        }
    }
    (invalid, non_https)
}

pub fn apply_tab_plan(
    browser: &mut Browser,
    plan: TabPlan,
    source: &str,
    now_ms: u64,
) -> Result<(WorkspaceId, usize), CommandError> {
    browser.apply(
        Command::AddWorkspace {
            name: format!("Imported from {source}"),
            color: GroupColor::Blue,
        },
        now_ms,
    )?;
    let workspace = browser
        .workspaces()
        .last()
        .map(|space| space.id())
        .ok_or(CommandError::UnknownWorkspace)?;
    browser.apply(Command::SwitchWorkspace(workspace), now_ms)?;
    let mut count = 0;
    for group in plan.groups {
        let mut tabs = Vec::new();
        for tab in group.tabs {
            browser.apply(
                Command::OpenTab {
                    url: tab.url,
                    placement: Placement::End,
                    activate: false,
                    opener: None,
                },
                now_ms,
            )?;
            let id = browser
                .workspace(workspace)
                .and_then(|space| space.items().last())
                .and_then(|item| match item {
                    Item::Tab(id) => Some(*id),
                    _ => None,
                })
                .ok_or(CommandError::UnknownTab)?;
            browser.apply(
                Command::SetTitle {
                    tab: id,
                    title: tab.title,
                },
                now_ms,
            )?;
            tabs.push(id);
            count += 1;
        }
        if !tabs.is_empty() {
            browser.apply(
                Command::CreateGroup {
                    tabs,
                    name: group.name,
                    color: GroupColor::Blue,
                },
                now_ms,
            )?;
        }
    }
    Ok((workspace, count))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn csv_preview_counts_reasons_without_values() {
        let csv = b"url,username,password\nhttps://example.com,a,one\nhttp://example.com,b,two\ninvalid,c,three\n";
        let parsed = aegis_import::parse_password_csv(csv).unwrap();
        assert_eq!(parsed.logins.len(), 1);
        assert_eq!(preview_counts(&parsed), (1, 1));
    }

    #[test]
    fn changed_csv_is_kept_even_when_its_length_matches() {
        let path = std::env::temp_dir().join(format!(
            "aegis-import-{}-{}.csv",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"url,username,password\nhttps://example.com,a,one\n").unwrap();
        let mut imports = Imports::new();
        imports.load_csv(&path);
        assert!(imports.preview.is_some());
        imports.mark_imported();
        std::fs::write(&path, b"url,username,password\nhttps://example.com,a,two\n").unwrap();
        assert!(matches!(imports.delete_csv(), Err(DeleteCsvError::Changed)));
        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn lock_drops_password_import_state() {
        let mut imports = Imports::new();
        imports.preview = Some(
            aegis_import::parse_password_csv(b"url,username,password\nhttps://example.com,a,one\n")
                .unwrap(),
        );
        imports.csv_path = Some(PathBuf::from("passwords.csv"));
        imports.delete_path = Some(PathBuf::from("older.csv"));
        imports.clear_passwords();
        assert!(imports.preview.is_none());
        assert!(imports.csv_path.is_none());
        assert!(imports.delete_path.is_none());
    }

    #[test]
    fn tab_plan_creates_unloaded_groups_in_fresh_browser() {
        let bookmarks = aegis_import::parse_netscape_html(b"<DL><p><DT><H3>Work</H3><DL><p><DT><A HREF=\"https://example.com\">Example</A></DL></DL>").unwrap();
        let plan = aegis_import::to_tab_plan(&bookmarks);
        let mut browser = Browser::new();
        let (workspace, count) = apply_tab_plan(&mut browser, plan, "HTML export", 1).unwrap();
        assert_eq!(count, 1);
        let space = browser.workspace(workspace).unwrap();
        assert!(matches!(&space.items()[0], Item::Group(group) if group.tabs().len() == 1));
        let id = match &space.items()[0] {
            Item::Group(group) => group.tabs()[0],
            _ => panic!("expected group"),
        };
        assert_eq!(
            browser.tab(id).unwrap().load(),
            aegis_core::LoadState::Unloaded
        );
    }
}
