use std::path::PathBuf;
use std::time::Duration;

use aegis_core::{Browser, session};

const DEBOUNCE_MS: u64 = 1000;

pub fn should_save(changed_at: Option<u64>, now_ms: u64) -> bool {
    changed_at.is_some_and(|changed| now_ms.saturating_sub(changed) >= DEBOUNCE_MS)
}

pub fn time_until_save(changed_at: Option<u64>, now_ms: u64) -> Option<Duration> {
    changed_at.map(|changed| {
        Duration::from_millis(DEBOUNCE_MS.saturating_sub(now_ms.saturating_sub(changed)))
    })
}

pub struct SessionSaver {
    path: PathBuf,
    changed_at: Option<u64>,
}

impl SessionSaver {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            changed_at: None,
        }
    }

    pub fn mark_dirty(&mut self, now_ms: u64) {
        self.changed_at = Some(now_ms);
    }

    pub fn time_until_due(&self, now_ms: u64) -> Option<Duration> {
        time_until_save(self.changed_at, now_ms)
    }

    pub fn save_if_due(
        &mut self,
        browser: &Browser,
        now_ms: u64,
    ) -> Result<(), session::SessionError> {
        if should_save(self.changed_at, now_ms) {
            self.save(browser)?;
        }
        Ok(())
    }

    pub fn save(&mut self, browser: &Browser) -> Result<(), session::SessionError> {
        session::save(&self.path, browser)?;
        self.changed_at = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{should_save, time_until_save};
    use std::time::Duration;

    #[test]
    fn debounces_last_change() {
        assert!(!should_save(None, 2000));
        assert!(!should_save(Some(100), 1099));
        assert!(should_save(Some(100), 1100));
        assert!(!should_save(Some(900), 1100));
    }

    #[test]
    fn time_until_save_tracks_last_change() {
        assert_eq!(time_until_save(None, 2000), None);
        assert_eq!(
            time_until_save(Some(100), 100),
            Some(Duration::from_millis(1000))
        );
        assert_eq!(
            time_until_save(Some(100), 1099),
            Some(Duration::from_millis(1))
        );
        assert_eq!(time_until_save(Some(100), 1100), Some(Duration::ZERO));
        assert_eq!(time_until_save(Some(100), 2000), Some(Duration::ZERO));
        assert_eq!(
            time_until_save(Some(900), 1100),
            Some(Duration::from_millis(800))
        );
    }
}
