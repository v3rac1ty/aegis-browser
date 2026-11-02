//! parsers for browser bookmark and password exports
#![forbid(unsafe_code)]

mod bookmarks;
mod discovery;
mod passwords;

pub use bookmarks::{
    ImportedBookmarks, ImportedFolder, ImportedItem, PlannedGroup, PlannedTab, TabPlan,
    parse_chromium, parse_netscape_html, parse_safari, to_tab_plan,
};
pub use discovery::{FoundProfile, Platform, Source, discover};
pub use passwords::{ImportedLogin, PasswordImport, SkipReason, parse_password_csv};

pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 100_000;
pub const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportError {
    TooLarge,
    Malformed,
    MissingColumns,
}

pub fn check_file_size(len: u64) -> Result<(), ImportError> {
    if len > MAX_FILE_BYTES {
        return Err(ImportError::TooLarge);
    }
    Ok(())
}
