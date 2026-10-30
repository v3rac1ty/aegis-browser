//! app data paths
#![forbid(unsafe_code)]

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Aegis")
    }

    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library/Application Support/Aegis")
    }

    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local/share"))
            .join("aegis")
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}
