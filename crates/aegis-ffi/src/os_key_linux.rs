use std::collections::HashMap;

use aegis_vault::VaultError;
use libsecret::{Schema, SchemaAttributeType, SchemaFlags, Service, ServiceFlags};
use wry::raw_window_handle::HasWindowHandle;
use zeroize::Zeroizing;

use super::OwnerCheck;
use crate::FfiError;

const REFERENCE_PREFIX: &[u8] = b"secret-service:v2:";
const HEX: &[u8; 16] = b"0123456789abcdef";

fn schema() -> Schema {
    let mut fields = HashMap::new();
    fields.insert("application", SchemaAttributeType::String);
    fields.insert("kind", SchemaAttributeType::String);
    fields.insert("id", SchemaAttributeType::String);
    Schema::new("app.aegis.vault", SchemaFlags::NONE, fields)
}

fn attributes(id: &str) -> HashMap<&str, &str> {
    HashMap::from([("application", "aegis"), ("kind", "vault-key"), ("id", id)])
}

fn service_available() -> Result<(), VaultError> {
    Service::sync(ServiceFlags::NONE, None::<&gtk::gio::Cancellable>)
        .map(|_| ())
        .map_err(|_| VaultError::KeystoreUnavailable)
}

fn encode(bytes: &[u8]) -> Zeroizing<String> {
    let mut text = Zeroizing::new(String::with_capacity(bytes.len() * 2));
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    text
}

fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn decode(text: &str) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(VaultError::Corrupt);
    }
    let mut result = Zeroizing::new(Vec::with_capacity(bytes.len() / 2));
    for pair in bytes.chunks_exact(2) {
        let high = nibble(pair[0]).ok_or(VaultError::Corrupt)?;
        let low = nibble(pair[1]).ok_or(VaultError::Corrupt)?;
        result.push((high << 4) | low);
    }
    Ok(result)
}

pub(super) fn seal(plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    service_available()?;
    let id = super::new_item_id()?;
    let encoded = encode(plaintext);
    libsecret::password_store_sync(
        Some(&schema()),
        attributes(&id),
        Some(libsecret::COLLECTION_DEFAULT.as_str()),
        "Aegis vault key",
        &encoded,
        None::<&gtk::gio::Cancellable>,
    )
    .map_err(|error| VaultError::Keystore(error.to_string()))?;
    // leave earlier items intact for other vault files and backups
    let mut reference = REFERENCE_PREFIX.to_vec();
    reference.extend_from_slice(id.as_bytes());
    Ok(reference)
}

pub(super) fn unseal(sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let id = super::parse_item_id(sealed, REFERENCE_PREFIX)?;
    service_available()?;
    let encoded = libsecret::password_lookup_sync(
        Some(&schema()),
        attributes(id),
        None::<&gtk::gio::Cancellable>,
    )
    .map_err(|error| VaultError::Keystore(error.to_string()))?
    .ok_or(VaultError::KeystoreUnavailable)?;
    let result = decode(&encoded);
    // SAFETY: Libsecret returned an owned writable GString that remains live until it is dropped.
    unsafe { std::ptr::write_bytes(encoded.as_ptr().cast_mut(), 0, encoded.len()) };
    result
}

// linux has no standard owner prompt; the app asks for the vault master password
pub(super) fn verify_owner(
    _window: &impl HasWindowHandle,
    _reason: &str,
    done: Box<dyn FnOnce(OwnerCheck) + Send>,
) -> Result<(), FfiError> {
    done(OwnerCheck::Unavailable);
    Ok(())
}

pub(super) fn owner_check_available() -> bool {
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reference_and_hex_mapping() {
        assert_eq!(&*encode(&[0, 15, 255]), "000fff");
        assert_eq!(&*decode("000fff").unwrap(), &[0, 15, 255]);
        assert!(matches!(decode("0g"), Err(VaultError::Corrupt)));
    }

    #[test]
    #[ignore = "needs a Secret Service session bus"]
    fn secret_service_round_trip() {
        let first = Zeroizing::new(vec![42; 32]);
        let second = Zeroizing::new(vec![43; 32]);
        let first_ref = seal(&first).unwrap();
        let second_ref = seal(&second).unwrap();
        assert!(first_ref.starts_with(REFERENCE_PREFIX));
        assert!(second_ref.starts_with(REFERENCE_PREFIX));
        assert_ne!(first_ref, second_ref);
        assert_eq!(*unseal(&first_ref).unwrap(), *first);
        assert_eq!(*unseal(&second_ref).unwrap(), *second);
    }
}
