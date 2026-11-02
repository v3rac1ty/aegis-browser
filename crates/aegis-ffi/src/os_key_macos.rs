use std::ffi::c_void;
use std::sync::Mutex;

use aegis_vault::VaultError;
use block2::RcBlock;
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2_foundation::{NSData, NSMutableData, NSMutableDictionary, NSNumber, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};
use objc2_security::{
    errSecItemNotFound, kSecAttrAccount, kSecAttrService, kSecAttrSynchronizable, kSecClass,
    kSecClassGenericPassword, kSecMatchLimit, kSecMatchLimitOne, kSecReturnData, kSecValueData,
};
use wry::raw_window_handle::HasWindowHandle;
use zeroize::Zeroizing;

use super::OwnerCheck;
use crate::FfiError;

const REFERENCE_PREFIX: &[u8] = b"keychain:v2:";

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecItemAdd(attributes: *const c_void, result: *mut *const c_void) -> i32;
    fn SecItemCopyMatching(query: *const c_void, result: *mut *const c_void) -> i32;
}

fn ns_key<T>(value: &'static T) -> &'static NSString {
    // SAFETY: Security framework CFString constants are toll-free bridged NSString objects.
    unsafe { &*(value as *const T as *const NSString) }
}

fn as_object<T: Message>(value: &T) -> &AnyObject {
    // SAFETY: Every Objective-C Message value has the same object pointer as its AnyObject base.
    unsafe { &*(value as *const T as *const AnyObject) }
}

type Query = NSMutableDictionary<NSString, AnyObject>;

fn query(id: &str) -> Retained<Query> {
    let query = Query::new();
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let class = unsafe { ns_key(kSecClass) };
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let generic = unsafe { ns_key(kSecClassGenericPassword) };
    query.insert(class, as_object(generic));
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let service = unsafe { ns_key(kSecAttrService) };
    query.insert(service, as_object(&*NSString::from_str("app.aegis.vault")));
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let account = unsafe { ns_key(kSecAttrAccount) };
    query.insert(
        account,
        as_object(&*NSString::from_str(&format!("vault-key-{id}"))),
    );
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let sync = unsafe { ns_key(kSecAttrSynchronizable) };
    query.insert(sync, as_object(&*NSNumber::numberWithBool(false)));
    query
}

fn status_error(status: i32) -> VaultError {
    if status == errSecItemNotFound {
        VaultError::KeystoreUnavailable
    } else {
        VaultError::Keystore(format!("keychain status {status}"))
    }
}

pub(super) fn seal(plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    // the login keychain binds items to the creating app's code identity and prompts other apps
    // signed release builds may use the data-protection keychain in M12
    let id = super::new_item_id()?;
    let add_query = query(&id);
    let data = NSMutableData::with_bytes(plaintext);
    // SAFETY: Security framework exports this constant CFString reference for process lifetime.
    let value_data = unsafe { ns_key(kSecValueData) };
    add_query.insert(value_data, as_object(&*data));
    // SAFETY: The dictionary is a live toll-free bridged CFDictionary and the null result is permitted.
    let status = unsafe {
        SecItemAdd(
            &*add_query as *const Query as *const c_void,
            std::ptr::null_mut(),
        )
    };
    // SAFETY: The data is mutable and no other access occurs while its bytes are cleared.
    unsafe { data.as_mut_bytes_unchecked().fill(0) };
    if status != 0 {
        return Err(status_error(status));
    }
    // leave earlier items intact for other vault files and backups
    let mut reference = REFERENCE_PREFIX.to_vec();
    reference.extend_from_slice(id.as_bytes());
    Ok(reference)
}

pub(super) fn unseal(sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let id = super::parse_item_id(sealed, REFERENCE_PREFIX)?;
    let query = query(id);
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let return_data = unsafe { ns_key(kSecReturnData) };
    query.insert(return_data, as_object(&*NSNumber::numberWithBool(true)));
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let match_limit = unsafe { ns_key(kSecMatchLimit) };
    // SAFETY: Security framework exports these constant CFString references for process lifetime.
    let one = unsafe { ns_key(kSecMatchLimitOne) };
    query.insert(match_limit, as_object(one));
    let mut result: *const c_void = std::ptr::null();
    // SAFETY: The query is a live toll-free bridged CFDictionary and result is a valid output pointer.
    let status =
        unsafe { SecItemCopyMatching(&*query as *const Query as *const c_void, &mut result) };
    if status != 0 {
        return Err(status_error(status));
    }
    // SAFETY: A successful data query returns a retained CFData, which is toll-free bridged to NSData.
    let data = unsafe { Retained::<NSData>::from_raw(result as *mut NSData) }
        .ok_or(VaultError::KeystoreUnavailable)?;
    // immutable keychain data keeps a plaintext copy until release; its const bytes cannot be safely wiped
    Ok(Zeroizing::new(data.to_vec()))
}

fn map_auth(success: bool) -> OwnerCheck {
    if success {
        OwnerCheck::Verified
    } else {
        OwnerCheck::Denied
    }
}

pub(super) fn verify_owner(
    _window: &impl HasWindowHandle,
    reason: &str,
    done: Box<dyn FnOnce(OwnerCheck) + Send>,
) -> Result<(), FfiError> {
    // SAFETY: LAContext is initialized by its documented new constructor.
    let context = unsafe { LAContext::new() };
    // SAFETY: The context is live and the error result is managed by objc2.
    if unsafe { context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) }.is_err() {
        done(OwnerCheck::Unavailable);
        return Ok(());
    }
    let reason = NSString::from_str(reason);
    let retained = Retained::into_raw(context.clone()) as usize;
    let pending = Mutex::new(Some((done, retained)));
    let reply = RcBlock::new(
        move |success: Bool, _error: *mut objc2_foundation::NSError| {
            if let Ok(mut pending) = pending.lock() {
                if let Some((done, retained)) = pending.take() {
                    // SAFETY: The retained context pointer is consumed exactly once by this completion block.
                    unsafe { drop(Retained::<LAContext>::from_raw(retained as *mut LAContext)) };
                    done(map_auth(success.into()));
                }
            }
        },
    );
    // SAFETY: The reply block owns Send state and remains valid until the framework calls it.
    unsafe {
        context.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthentication,
            &reason,
            &reply,
        );
    }
    Ok(())
}

pub(super) fn owner_check_available() -> bool {
    // SAFETY: LAContext is initialized by its documented new constructor.
    let context = unsafe { LAContext::new() };
    // SAFETY: The context is live and canEvaluatePolicy does not prompt.
    unsafe { context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) }.is_ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn auth_mapping() {
        assert_eq!(map_auth(true), OwnerCheck::Verified);
        assert_eq!(map_auth(false), OwnerCheck::Denied);
    }

    #[test]
    #[ignore = "needs a login keychain"]
    fn keychain_round_trip() {
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
