#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

const VERSION: u8 = 1;
const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_ITERATIONS: u32 = 3;
const KDF_PARALLELISM: u32 = 1;

#[derive(Debug)]
pub enum VaultError {
    Locked,
    Corrupt,
    InsecureOrigin,
    InvalidOrigin,
    NotFound,
    ExhaustedIds,
    Random,
    Io(std::io::Error),
    Json(serde_json::Error),
    Protector,
    KeystoreUnavailable,
    Keystore(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Locked => write!(f, "vault is locked"),
            Self::Corrupt => write!(f, "vault is corrupt"),
            Self::InsecureOrigin => write!(f, "origin must use https"),
            Self::InvalidOrigin => write!(f, "invalid origin"),
            Self::NotFound => write!(f, "entry not found"),
            Self::ExhaustedIds => write!(f, "entry ids exhausted"),
            Self::Random => write!(f, "randomness unavailable"),
            Self::Io(error) => write!(f, "vault file: {error}"),
            Self::Json(error) => write!(f, "vault json: {error}"),
            Self::Protector => write!(f, "key protector failed"),
            Self::KeystoreUnavailable => write!(f, "os keystore unavailable"),
            Self::Keystore(message) => write!(f, "os keystore: {message}"),
        }
    }
}

impl std::error::Error for VaultError {}

/// seals and unseals vault key material using platform storage
pub trait KeyProtector {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError>;
    fn unseal(&self, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntryId(u64);

impl EntryId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMeta {
    pub id: EntryId,
    pub origin: String,
    pub username: String,
    pub created_ms: u64,
    pub updated_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct KdfInfo {
    pub params: KdfParams,
    pub salt: [u8; 16],
}

impl std::fmt::Debug for KdfInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KdfInfo")
            .field("params", &self.params)
            .field("salt", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EntryFile {
    pub id: EntryId,
    pub meta_nonce: [u8; 24],
    pub meta_ct: Vec<u8>,
    pub pw_nonce: [u8; 24],
    pub pw_ct: Vec<u8>,
}

impl std::fmt::Debug for EntryFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryFile")
            .field("id", &self.id)
            .field("ciphertext", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VaultFile {
    pub version: u8,
    pub kdf: Option<KdfInfo>,
    #[serde(rename = "sealed_key")]
    pub protector_sealed: Vec<u8>,
    pub entries: Vec<EntryFile>,
}

impl std::fmt::Debug for VaultFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultFile")
            .field("version", &self.version)
            .field("kdf", &self.kdf)
            .field("sealed_key", &"[redacted]")
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

struct Entry {
    meta: EntryMeta,
    encrypted: EntryFile,
}

pub struct Vault {
    key: Zeroizing<[u8; 32]>,
    kdf: Option<KdfInfo>,
    protector_sealed: Vec<u8>,
    entries: Vec<Entry>,
    next_id: u64,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("key", &"[redacted]")
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

impl Vault {
    pub fn create(
        protector: &dyn KeyProtector,
        master: Option<&str>,
    ) -> Result<(Vault, VaultFile), VaultError> {
        let mut key = Zeroizing::new([0; 32]);
        getrandom::fill(&mut *key).map_err(|_| VaultError::Random)?;
        let (kdf, protector_sealed) = if let Some(master) = master {
            let mut salt = [0; 16];
            getrandom::fill(&mut salt).map_err(|_| VaultError::Random)?;
            let info = KdfInfo {
                params: KdfParams {
                    memory_kib: KDF_MEMORY_KIB,
                    iterations: KDF_ITERATIONS,
                    parallelism: KDF_PARALLELISM,
                },
                salt,
            };
            let kek = derive_kek(master, &info)?;
            let (wrap_nonce, wrapped_key) = encrypt(&kek, &key[..], b"aegis-vault-kek-v1")?;
            let mut bundle = Zeroizing::new(Vec::with_capacity(24 + wrapped_key.len()));
            bundle.extend_from_slice(&wrap_nonce);
            bundle.extend_from_slice(&wrapped_key);
            let sealed = protector.seal(&bundle)?;
            (Some(info), sealed)
        } else {
            (None, protector.seal(&key[..])?)
        };
        let vault = Self {
            key,
            kdf,
            protector_sealed,
            entries: Vec::new(),
            next_id: 1,
        };
        let file = vault.to_file();
        Ok((vault, file))
    }

    pub fn open(
        file: &VaultFile,
        protector: &dyn KeyProtector,
        master: Option<&str>,
    ) -> Result<Vault, VaultError> {
        if file.version != VERSION {
            return Err(VaultError::Corrupt);
        }
        let unsealed = protector.unseal(&file.protector_sealed)?;
        let mut key = Zeroizing::new([0; 32]);
        match (&file.kdf, master) {
            (Some(info), Some(master)) => {
                if info.params.memory_kib != KDF_MEMORY_KIB
                    || info.params.iterations != KDF_ITERATIONS
                    || info.params.parallelism != KDF_PARALLELISM
                    || unsealed.len() != 24 + 32 + 16
                {
                    return Err(VaultError::Corrupt);
                }
                let nonce: &[u8; 24] =
                    unsealed[..24].try_into().map_err(|_| VaultError::Corrupt)?;
                let kek = derive_kek(master, info)?;
                let plaintext = Zeroizing::new(
                    decrypt(&kek, nonce, &unsealed[24..], b"aegis-vault-kek-v1")
                        .map_err(|_| VaultError::Locked)?,
                );
                key.copy_from_slice(&plaintext);
            }
            (None, None) => {
                if unsealed.len() != 32 {
                    return Err(VaultError::Corrupt);
                }
                key.copy_from_slice(&unsealed);
            }
            _ => return Err(VaultError::Locked),
        }
        let mut entries = Vec::with_capacity(file.entries.len());
        let mut seen = HashSet::with_capacity(file.entries.len());
        let mut next_id = 1;
        for encrypted in &file.entries {
            if !seen.insert(encrypted.id) || encrypted.id.0 == 0 {
                return Err(VaultError::Corrupt);
            }
            let bytes = Zeroizing::new(decrypt(
                &key,
                &encrypted.meta_nonce,
                &encrypted.meta_ct,
                &meta_aad(encrypted.id),
            )?);
            let meta: EntryMeta =
                serde_json::from_slice(&bytes).map_err(|_| VaultError::Corrupt)?;
            if meta.id != encrypted.id
                || normalize_origin(&meta.origin).ok().as_deref() != Some(&meta.origin)
            {
                return Err(VaultError::Corrupt);
            }
            next_id = next_id.max(encrypted.id.0.saturating_add(1));
            entries.push(Entry {
                meta,
                encrypted: encrypted.clone(),
            });
        }
        Ok(Self {
            key,
            kdf: file.kdf.clone(),
            protector_sealed: file.protector_sealed.clone(),
            entries,
            next_id,
        })
    }

    pub fn add(
        &mut self,
        origin: &str,
        username: &str,
        password: &str,
        now_ms: u64,
    ) -> Result<EntryId, VaultError> {
        let origin = normalize_origin(origin)?;
        let id = EntryId(self.next_id);
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(VaultError::ExhaustedIds)?;
        let meta = EntryMeta {
            id,
            origin,
            username: username.to_owned(),
            created_ms: now_ms,
            updated_ms: now_ms,
        };
        let encrypted = encrypt_entry(&self.key, &meta, password)?;
        self.entries.push(Entry { meta, encrypted });
        self.next_id = next_id;
        Ok(id)
    }

    pub fn update_password(
        &mut self,
        id: EntryId,
        password: &str,
        now_ms: u64,
    ) -> Result<(), VaultError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.meta.id == id)
            .ok_or(VaultError::NotFound)?;
        let mut meta = entry.meta.clone();
        meta.updated_ms = now_ms;
        let encrypted = encrypt_entry(&self.key, &meta, password)?;
        entry.meta = meta;
        entry.encrypted = encrypted;
        Ok(())
    }

    pub fn remove(&mut self, id: EntryId) -> Result<(), VaultError> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.meta.id == id)
            .ok_or(VaultError::NotFound)?;
        self.entries.remove(index);
        Ok(())
    }

    pub fn list(&self) -> Vec<EntryMeta> {
        self.entries
            .iter()
            .map(|entry| entry.meta.clone())
            .collect()
    }

    pub fn for_origin(&self, origin: &str) -> Vec<EntryMeta> {
        let Ok(origin) = normalize_origin(origin) else {
            return Vec::new();
        };
        self.entries
            .iter()
            .filter(|entry| entry.meta.origin == origin)
            .map(|entry| entry.meta.clone())
            .collect()
    }

    pub fn reveal(&self, id: EntryId) -> Result<Zeroizing<String>, VaultError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.meta.id == id)
            .ok_or(VaultError::NotFound)?;
        let bytes = Zeroizing::new(decrypt(
            &self.key,
            &entry.encrypted.pw_nonce,
            &entry.encrypted.pw_ct,
            &password_aad(&entry.meta),
        )?);
        let password = std::str::from_utf8(&bytes).map_err(|_| VaultError::Corrupt)?;
        Ok(Zeroizing::new(password.to_owned()))
    }

    pub fn to_file(&self) -> VaultFile {
        VaultFile {
            version: VERSION,
            kdf: self.kdf.clone(),
            protector_sealed: self.protector_sealed.clone(),
            entries: self
                .entries
                .iter()
                .map(|entry| entry.encrypted.clone())
                .collect(),
        }
    }

    /// replaces the key wrap without changing encrypted entries
    pub fn rewrap(
        &mut self,
        protector: &dyn KeyProtector,
        master: Option<&str>,
    ) -> Result<VaultFile, VaultError> {
        let (kdf, sealed) = if let Some(master) = master {
            let mut salt = [0; 16];
            getrandom::fill(&mut salt).map_err(|_| VaultError::Random)?;
            let info = KdfInfo {
                params: KdfParams {
                    memory_kib: KDF_MEMORY_KIB,
                    iterations: KDF_ITERATIONS,
                    parallelism: KDF_PARALLELISM,
                },
                salt,
            };
            let kek = derive_kek(master, &info)?;
            let (nonce, wrapped) = encrypt(&kek, &self.key[..], b"aegis-vault-kek-v1")?;
            let mut bundle = Zeroizing::new(Vec::with_capacity(nonce.len() + wrapped.len()));
            bundle.extend_from_slice(&nonce);
            bundle.extend_from_slice(&wrapped);
            (Some(info), protector.seal(&bundle)?)
        } else {
            (None, protector.seal(&self.key[..])?)
        };
        self.kdf = kdf;
        self.protector_sealed = sealed;
        Ok(self.to_file())
    }
}

fn derive_kek(master: &str, info: &KdfInfo) -> Result<Zeroizing<[u8; 32]>, VaultError> {
    let params = Params::new(
        info.params.memory_kib,
        info.params.iterations,
        info.params.parallelism,
        Some(32),
    )
    .map_err(|_| VaultError::Corrupt)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut kek = Zeroizing::new([0; 32]);
    argon
        .hash_password_into(master.as_bytes(), &info.salt, &mut *kek)
        .map_err(|_| VaultError::Corrupt)?;
    Ok(kek)
}

fn encrypt_entry(
    key: &[u8; 32],
    meta: &EntryMeta,
    password: &str,
) -> Result<EntryFile, VaultError> {
    let meta_bytes = Zeroizing::new(serde_json::to_vec(meta).map_err(VaultError::Json)?);
    let (meta_nonce, meta_ct) = encrypt(key, &meta_bytes, &meta_aad(meta.id))?;
    let (pw_nonce, pw_ct) = encrypt(key, password.as_bytes(), &password_aad(meta))?;
    Ok(EntryFile {
        id: meta.id,
        meta_nonce,
        meta_ct,
        pw_nonce,
        pw_ct,
    })
}

fn encrypt(
    key: &[u8; 32],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<([u8; 24], Vec<u8>), VaultError> {
    let mut nonce = [0; 24];
    getrandom::fill(&mut nonce).map_err(|_| VaultError::Random)?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VaultError::Corrupt)?;
    Ok((nonce, ciphertext))
}

fn decrypt(
    key: &[u8; 32],
    nonce: &[u8; 24],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, VaultError> {
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| VaultError::Corrupt)
}

fn meta_aad(id: EntryId) -> Vec<u8> {
    let mut aad = b"aegis-vault-meta-v1".to_vec();
    aad.extend_from_slice(&id.0.to_le_bytes());
    aad
}

fn password_aad(meta: &EntryMeta) -> Vec<u8> {
    let mut aad = b"aegis-vault-password-v1".to_vec();
    aad.extend_from_slice(&meta.id.0.to_le_bytes());
    aad.extend_from_slice(&(meta.origin.len() as u64).to_le_bytes());
    aad.extend_from_slice(meta.origin.as_bytes());
    aad.extend_from_slice(&(meta.username.len() as u64).to_le_bytes());
    aad.extend_from_slice(meta.username.as_bytes());
    aad
}

fn normalize_origin(input: &str) -> Result<String, VaultError> {
    let (scheme, rest) = input.split_once("://").ok_or(VaultError::InvalidOrigin)?;
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(VaultError::InsecureOrigin);
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .ok_or(VaultError::InvalidOrigin)?;
    if authority.is_empty() || authority.contains('@') || authority.chars().any(char::is_whitespace)
    {
        return Err(VaultError::InvalidOrigin);
    }
    let (host, port) = if authority.starts_with('[') {
        let close = authority.find(']').ok_or(VaultError::InvalidOrigin)?;
        let host = &authority[..=close];
        let tail = &authority[close + 1..];
        if tail.is_empty() {
            (host, None)
        } else {
            (
                host,
                Some(tail.strip_prefix(':').ok_or(VaultError::InvalidOrigin)?),
            )
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty()
        || host.contains(':') && !host.starts_with('[')
        || host.contains(']') && !host.starts_with('[')
    {
        return Err(VaultError::InvalidOrigin);
    }
    let host = host.to_ascii_lowercase();
    match port {
        None | Some("443") => Ok(format!("https://{host}")),
        Some(value) => {
            let port: u16 = value.parse().map_err(|_| VaultError::InvalidOrigin)?;
            if port == 0 {
                return Err(VaultError::InvalidOrigin);
            }
            if port == 443 {
                Ok(format!("https://{host}"))
            } else {
                Ok(format!("https://{host}:{port}"))
            }
        }
    }
}

/// writes a synced temporary file and keeps the prior valid file as a backup
pub fn save(path: &Path, file: &VaultFile) -> Result<(), VaultError> {
    let tmp = suffixed(path, ".tmp");
    let backup = suffixed(path, ".bak");
    let mut writer = BufWriter::new(File::create(&tmp).map_err(VaultError::Io)?);
    serde_json::to_writer(&mut writer, file).map_err(VaultError::Json)?;
    writer.flush().map_err(VaultError::Io)?;
    let handle = writer
        .into_inner()
        .map_err(|error| VaultError::Io(error.into_error()))?;
    handle.sync_all().map_err(VaultError::Io)?;
    drop(handle);
    if load_one(path).is_ok() {
        fs::copy(path, backup).map_err(VaultError::Io)?;
    }
    fs::rename(tmp, path).map_err(VaultError::Io)?;
    Ok(())
}

/// loads the main file or a structurally valid backup
pub fn load(path: &Path) -> Result<VaultFile, VaultError> {
    match load_one(path) {
        Ok(file) => Ok(file),
        Err(error) => {
            let backup = suffixed(path, ".bak");
            if backup.exists() {
                load_one(&backup)
            } else {
                Err(error)
            }
        }
    }
}

fn load_one(path: &Path) -> Result<VaultFile, VaultError> {
    let bytes = fs::read(path).map_err(VaultError::Io)?;
    let file: VaultFile = serde_json::from_slice(&bytes).map_err(VaultError::Json)?;
    if file.version != VERSION {
        return Err(VaultError::Corrupt);
    }
    Ok(file)
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
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeProtector;

    impl KeyProtector for FakeProtector {
        fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
            Ok(plaintext.iter().map(|byte| byte ^ 0x5a).collect())
        }

        fn unseal(&self, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
            Ok(Zeroizing::new(
                sealed.iter().map(|byte| byte ^ 0x5a).collect(),
            ))
        }
    }

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn test_dir() -> PathBuf {
        let unique = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("aegis-vault-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn round_trip_add_save_load_open_reveal() {
        let dir = test_dir();
        let path = dir.join("vault.json");
        let (mut vault, _) = Vault::create(&FakeProtector, Some("master")).unwrap();
        let id = vault
            .add("HTTPS://Example.COM:443/login", "alice", "secret", 10)
            .unwrap();
        assert_eq!(vault.list()[0].origin, "https://example.com");
        save(&path, &vault.to_file()).unwrap();
        let opened = Vault::open(&load(&path).unwrap(), &FakeProtector, Some("master")).unwrap();
        assert_eq!(&*opened.reveal(id).unwrap(), "secret");
        assert_eq!(opened.list()[0].created_ms, 10);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wrong_master_is_locked_even_when_empty() {
        let (_, file) = Vault::create(&FakeProtector, Some("right")).unwrap();
        assert!(matches!(
            Vault::open(&file, &FakeProtector, Some("wrong")),
            Err(VaultError::Locked)
        ));
    }

    #[test]
    fn rewrap_round_trip_preserves_entries_and_rotates_salt() {
        let (mut vault, _) = Vault::create(&FakeProtector, Some("old")).unwrap();
        let id = vault
            .add("https://example.com", "alice", "secret", 1)
            .unwrap();
        let old = vault.to_file();
        let changed = vault.rewrap(&FakeProtector, Some("new")).unwrap();
        assert_ne!(old.kdf.unwrap().salt, changed.kdf.as_ref().unwrap().salt);
        assert_eq!(old.entries[0].pw_ct, changed.entries[0].pw_ct);
        assert!(matches!(
            Vault::open(&changed, &FakeProtector, Some("old")),
            Err(VaultError::Locked)
        ));
        let reopened = Vault::open(&changed, &FakeProtector, Some("new")).unwrap();
        assert!(
            reopened
                .reveal(id)
                .is_ok_and(|value| value.as_str() == "secret")
        );
        let no_master = vault.rewrap(&FakeProtector, None).unwrap();
        let reopened = Vault::open(&no_master, &FakeProtector, None).unwrap();
        assert!(
            reopened
                .reveal(id)
                .is_ok_and(|value| value.as_str() == "secret")
        );
    }

    #[test]
    fn master_wrap_has_nonce_and_authenticated_key() {
        let (_, file) = Vault::create(&FakeProtector, Some("master")).unwrap();
        assert_eq!(file.version, 1);
        assert_eq!(file.protector_sealed.len(), 24 + 32 + 16);
        let json = serde_json::to_value(&file).unwrap();
        assert!(json["kdf"].get("verifier").is_none());

        let mut tampered = file.clone();
        tampered.protector_sealed[24] ^= 1;
        assert!(matches!(
            Vault::open(&tampered, &FakeProtector, Some("master")),
            Err(VaultError::Locked)
        ));
    }

    #[test]
    fn flipping_password_ciphertext_corrupts_only_that_entry() {
        let (mut vault, _) = Vault::create(&FakeProtector, None).unwrap();
        let first = vault.add("https://a.com", "a", "first", 1).unwrap();
        let second = vault.add("https://b.com", "b", "second", 1).unwrap();
        let mut file = vault.to_file();
        file.entries[0].pw_ct[0] ^= 1;
        let opened = Vault::open(&file, &FakeProtector, None).unwrap();
        assert!(matches!(opened.reveal(first), Err(VaultError::Corrupt)));
        assert_eq!(&*opened.reveal(second).unwrap(), "second");
    }

    #[test]
    fn swapping_password_ciphertexts_is_corrupt() {
        let (mut vault, _) = Vault::create(&FakeProtector, None).unwrap();
        let first = vault.add("https://a.com", "a", "same", 1).unwrap();
        let second = vault.add("https://b.com", "b", "same", 1).unwrap();
        let mut file = vault.to_file();
        let (left, right) = file.entries.split_at_mut(1);
        std::mem::swap(&mut left[0].pw_ct, &mut right[0].pw_ct);
        let opened = Vault::open(&file, &FakeProtector, None).unwrap();
        assert!(matches!(opened.reveal(first), Err(VaultError::Corrupt)));
        assert!(matches!(opened.reveal(second), Err(VaultError::Corrupt)));
    }

    #[test]
    fn exact_https_origin_matching() {
        let (mut vault, _) = Vault::create(&FakeProtector, None).unwrap();
        vault.add("https://a.com/login", "a", "one", 1).unwrap();
        vault.add("https://a.com:8443", "b", "two", 1).unwrap();
        vault.add("https://sub.a.com", "c", "three", 1).unwrap();
        assert_eq!(vault.for_origin("https://A.COM:443/other").len(), 1);
        assert_eq!(vault.for_origin("https://a.com:8443").len(), 1);
        assert_eq!(vault.for_origin("https://sub.a.com").len(), 1);
        assert!(vault.for_origin("http://a.com").is_empty());
        assert!(matches!(
            vault.add("http://a.com", "x", "x", 1),
            Err(VaultError::InsecureOrigin)
        ));
    }

    #[test]
    fn serialized_file_has_no_plaintext_fields() {
        let (mut vault, _) = Vault::create(&FakeProtector, None).unwrap();
        vault
            .add(
                "https://private.example",
                "unique-user",
                "unique-password",
                1,
            )
            .unwrap();
        let bytes = serde_json::to_vec(&vault.to_file()).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("private.example"));
        assert!(!text.contains("unique-user"));
        assert!(!text.contains("unique-password"));
    }

    #[test]
    fn argon2_params_are_stored() {
        let (_, file) = Vault::create(&FakeProtector, Some("master")).unwrap();
        let info = file.kdf.unwrap();
        assert_eq!(info.params.memory_kib, 64 * 1024);
        assert_eq!(info.params.iterations, 3);
        assert_eq!(info.params.parallelism, 1);
        assert_eq!(info.salt.len(), 16);
    }

    #[test]
    fn keystore_errors_are_distinct() {
        assert_eq!(
            VaultError::KeystoreUnavailable.to_string(),
            "os keystore unavailable"
        );
        assert_eq!(
            VaultError::Keystore("access denied".into()).to_string(),
            "os keystore: access denied"
        );
    }

    #[test]
    fn update_remove_and_backup() {
        let dir = test_dir();
        let path = dir.join("vault.json");
        let (mut vault, _) = Vault::create(&FakeProtector, None).unwrap();
        let id = vault.add("https://a.com", "alice", "old", 1).unwrap();
        save(&path, &vault.to_file()).unwrap();
        vault.update_password(id, "new", 2).unwrap();
        save(&path, &vault.to_file()).unwrap();
        let prior = Vault::open(
            &load_one(&suffixed(&path, ".bak")).unwrap(),
            &FakeProtector,
            None,
        )
        .unwrap();
        assert_eq!(&*prior.reveal(id).unwrap(), "old");
        assert_eq!(
            &*Vault::open(&load(&path).unwrap(), &FakeProtector, None)
                .unwrap()
                .reveal(id)
                .unwrap(),
            "new"
        );
        vault.remove(id).unwrap();
        assert!(vault.list().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }
}
