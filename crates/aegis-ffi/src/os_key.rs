use aegis_vault::{KeyProtector, VaultError};
use wry::raw_window_handle::HasWindowHandle;
use zeroize::Zeroizing;

use crate::FfiError;

#[cfg(any(target_os = "macos", target_os = "linux"))]
const HEX: &[u8; 16] = b"0123456789abcdef";

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn new_item_id() -> Result<String, VaultError> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| VaultError::Random)?;
    let mut id = String::with_capacity(32);
    for byte in random {
        id.push(HEX[(byte >> 4) as usize] as char);
        id.push(HEX[(byte & 15) as usize] as char);
    }
    Ok(id)
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn parse_item_id<'a>(sealed: &'a [u8], prefix: &[u8]) -> Result<&'a str, VaultError> {
    let id = sealed.strip_prefix(prefix).ok_or(VaultError::Corrupt)?;
    if id.len() != 32 || !id.iter().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(VaultError::Corrupt);
    }
    std::str::from_utf8(id).map_err(|_| VaultError::Corrupt)
}

/// protects vault keys with the current user's os keystore
#[derive(Debug)]
pub struct OsKeyProtector;

/// result of a native owner prompt
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerCheck {
    Verified,
    Denied,
    Unavailable,
}

impl KeyProtector for OsKeyProtector {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
        platform::seal(plaintext)
    }

    fn unseal(&self, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        platform::unseal(sealed)
    }
}

/// asks the os to authenticate the window owner
pub fn verify_owner(
    window: &impl HasWindowHandle,
    reason: &str,
    done: Box<dyn FnOnce(OwnerCheck) + Send>,
) -> Result<(), FfiError> {
    if reason.trim().is_empty() {
        return Err(FfiError::Native("owner prompt reason is empty".into()));
    }
    platform::verify_owner(window, reason, done)
}

/// probes whether the os can authenticate without prompting
pub fn owner_check_available() -> bool {
    platform::owner_check_available()
}

#[cfg(target_os = "windows")]
#[path = "os_key_windows.rs"]
mod platform;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod reference_tests {
    use super::*;

    #[test]
    fn parses_v2_references() {
        let id = b"00112233445566778899aabbccddeeff";
        let mut reference = b"keychain:v2:".to_vec();
        reference.extend_from_slice(id);
        assert_eq!(
            parse_item_id(&reference, b"keychain:v2:").unwrap(),
            "00112233445566778899aabbccddeeff"
        );
        let mut reference = b"secret-service:v2:".to_vec();
        reference.extend_from_slice(id);
        assert_eq!(
            parse_item_id(&reference, b"secret-service:v2:").unwrap(),
            "00112233445566778899aabbccddeeff"
        );
    }

    #[test]
    fn rejects_wrong_prefix() {
        assert!(matches!(
            parse_item_id(
                b"secret-service:v2:00112233445566778899aabbccddeeff",
                b"keychain:v2:"
            ),
            Err(VaultError::Corrupt)
        ));
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(matches!(
            parse_item_id(
                b"keychain:v2:00112233445566778899aabbccddeefg",
                b"keychain:v2:"
            ),
            Err(VaultError::Corrupt)
        ));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(matches!(
            parse_item_id(b"keychain:v2:00112233", b"keychain:v2:"),
            Err(VaultError::Corrupt)
        ));
    }
}
#[cfg(target_os = "macos")]
#[path = "os_key_macos.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "os_key_linux.rs"]
mod platform;
