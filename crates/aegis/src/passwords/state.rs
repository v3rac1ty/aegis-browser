use std::path::PathBuf;
use std::time::{Duration, Instant};

use aegis_ffi::OsKeyProtector;
use aegis_import::ImportedLogin;
use aegis_vault::{EntryId, EntryMeta, KeyProtector, Vault, VaultError, VaultFile};
use zeroize::Zeroizing;

const AUTO_LOCK: Duration = Duration::from_secs(600);
const MASTER_ONLY: &[u8] = b"master-only:v1:";

struct MasterProtector;

impl KeyProtector for MasterProtector {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        let mut sealed = MASTER_ONLY.to_vec();
        sealed.extend_from_slice(plaintext);
        Ok(sealed)
    }

    fn unseal(&self, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        let bytes = sealed
            .strip_prefix(MASTER_ONLY)
            .ok_or(VaultError::Corrupt)?;
        Ok(Zeroizing::new(bytes.to_vec()))
    }
}

pub struct VaultState {
    path: PathBuf,
    file: Option<VaultFile>,
    vault: Option<Vault>,
    used_at: Option<Instant>,
    verified: bool,
    master_only: bool,
}

impl VaultState {
    pub fn unavailable(path: PathBuf) -> Self {
        Self {
            path,
            file: None,
            vault: None,
            used_at: None,
            verified: false,
            master_only: false,
        }
    }
    pub fn new(path: PathBuf) -> Result<Self, VaultError> {
        let file = if path.exists() {
            Some(aegis_vault::load(&path)?)
        } else {
            None
        };
        let master_only = file
            .as_ref()
            .is_some_and(|file| file.protector_sealed.starts_with(MASTER_ONLY));
        if master_only && file.as_ref().is_some_and(|file| file.kdf.is_none()) {
            return Err(VaultError::Corrupt);
        }
        let vault = match &file {
            Some(file) if file.kdf.is_none() && !master_only => {
                Some(Vault::open(file, &OsKeyProtector, None)?)
            }
            _ => None,
        };
        Ok(Self {
            path,
            file,
            vault,
            used_at: Some(Instant::now()),
            verified: false,
            master_only,
        })
    }

    pub fn exists(&self) -> bool {
        self.file.is_some()
    }
    pub fn locked(&self) -> bool {
        self.vault.is_none()
    }
    pub fn has_master(&self) -> bool {
        self.file.as_ref().is_some_and(|file| file.kdf.is_some())
    }
    pub fn os_protected(&self) -> bool {
        self.exists() && !self.master_only
    }
    pub fn verified(&self) -> bool {
        self.verified
    }
    pub fn mark_verified(&mut self) {
        self.verified = true;
        self.touch();
    }

    pub fn touch(&mut self) {
        self.used_at = Some(Instant::now());
    }

    pub fn auto_lock(&mut self, now: Instant) -> bool {
        if self.vault.is_some()
            && self
                .used_at
                .is_some_and(|at| now.saturating_duration_since(at) >= AUTO_LOCK)
        {
            self.lock();
            return true;
        }
        false
    }

    pub fn until_lock(&self, now: Instant) -> Option<Duration> {
        self.vault.as_ref()?;
        self.used_at
            .map(|at| AUTO_LOCK.saturating_sub(now.saturating_duration_since(at)))
    }

    pub fn lock(&mut self) {
        self.vault = None;
        self.used_at = None;
        self.verified = false;
    }

    pub fn create(&mut self, master: Option<&str>) -> Result<(), VaultError> {
        if self.exists() || self.path.exists() {
            return Err(VaultError::Corrupt);
        }
        let (vault, file, master_only) = match Vault::create(&OsKeyProtector, master) {
            Ok((vault, file)) => (vault, file, false),
            Err(VaultError::KeystoreUnavailable) => {
                let master = master.ok_or(VaultError::KeystoreUnavailable)?;
                let (vault, file) = Vault::create(&MasterProtector, Some(master))?;
                (vault, file, true)
            }
            Err(error) => return Err(error),
        };
        aegis_vault::save(&self.path, &file)?;
        self.vault = Some(vault);
        self.file = Some(file);
        self.master_only = master_only;
        self.touch();
        Ok(())
    }

    pub fn verify_master(&mut self, master: &str) -> Result<(), VaultError> {
        let file = self.file.as_ref().ok_or(VaultError::NotFound)?;
        let protector: &dyn KeyProtector = if self.master_only {
            &MasterProtector
        } else {
            &OsKeyProtector
        };
        let checked = Vault::open(file, protector, Some(master))?;
        if self.vault.is_none() {
            self.vault = Some(checked);
        }
        self.touch();
        Ok(())
    }

    pub fn open_without_master(&mut self) -> Result<(), VaultError> {
        if self.vault.is_some() {
            return Ok(());
        }
        let file = self.file.as_ref().ok_or(VaultError::NotFound)?;
        if file.kdf.is_some() {
            return Err(VaultError::Locked);
        }
        self.vault = Some(Vault::open(file, &OsKeyProtector, None)?);
        self.touch();
        Ok(())
    }

    pub fn list(&self) -> Vec<EntryMeta> {
        self.vault.as_ref().map_or_else(Vec::new, Vault::list)
    }

    pub fn for_origin(&self, origin: &str) -> Vec<EntryMeta> {
        self.vault
            .as_ref()
            .map_or_else(Vec::new, |vault| vault.for_origin(origin))
    }

    pub fn reveal(&mut self, id: EntryId) -> Result<Zeroizing<String>, VaultError> {
        self.touch();
        self.vault.as_ref().ok_or(VaultError::Locked)?.reveal(id)
    }

    pub fn add(
        &mut self,
        origin: &str,
        username: &str,
        password: &str,
        now_ms: u64,
    ) -> Result<(), VaultError> {
        self.touch();
        self.vault
            .as_mut()
            .ok_or(VaultError::Locked)?
            .add(origin, username, password, now_ms)?;
        self.save()
    }

    pub fn import(&mut self, logins: &[ImportedLogin], now_ms: u64) -> Result<usize, VaultError> {
        self.touch();
        let vault = self.vault.as_mut().ok_or(VaultError::Locked)?;
        let mut count = 0;
        for login in logins {
            vault.add(&login.origin, &login.username, &login.password, now_ms)?;
            count += 1;
        }
        self.save()?;
        Ok(count)
    }

    pub fn update_password(
        &mut self,
        id: EntryId,
        password: &str,
        now_ms: u64,
    ) -> Result<(), VaultError> {
        self.touch();
        self.vault
            .as_mut()
            .ok_or(VaultError::Locked)?
            .update_password(id, password, now_ms)?;
        self.save()
    }

    pub fn remove(&mut self, id: EntryId) -> Result<(), VaultError> {
        self.touch();
        self.vault.as_mut().ok_or(VaultError::Locked)?.remove(id)?;
        self.save()
    }

    pub fn rewrap(&mut self, master: Option<&str>) -> Result<(), VaultError> {
        let protector: &dyn KeyProtector = if self.master_only {
            &MasterProtector
        } else {
            &OsKeyProtector
        };
        if self.master_only && master.is_none() {
            return Err(VaultError::KeystoreUnavailable);
        }
        let file = self
            .vault
            .as_mut()
            .ok_or(VaultError::Locked)?
            .rewrap(protector, master)?;
        aegis_vault::save(&self.path, &file)?;
        self.file = Some(file);
        self.touch();
        Ok(())
    }

    fn save(&mut self) -> Result<(), VaultError> {
        let file = self.vault.as_ref().ok_or(VaultError::Locked)?.to_file();
        aegis_vault::save(&self.path, &file)?;
        self.file = Some(file);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_locks_after_ten_minutes_and_clears_owner_check() {
        let mut state = VaultState {
            path: PathBuf::new(),
            file: None,
            vault: None,
            used_at: None,
            verified: false,
            master_only: false,
        };
        let (vault, file) = Vault::create(&MasterProtector, Some("master")).unwrap();
        state.file = Some(file);
        state.vault = Some(vault);
        state.master_only = true;
        let at = Instant::now();
        state.mark_verified();
        state.used_at = Some(at);
        assert!(!state.auto_lock(at + Duration::from_secs(599)));
        assert!(state.auto_lock(at + Duration::from_secs(600)));
        assert!(state.locked());
        assert!(!state.verified());
        state.verify_master("master").unwrap();
        assert!(!state.locked());
    }
}
