use aegis_vault::VaultError;
use std::sync::{Arc, Mutex};
use windows::Security::Credentials::UI::{
    UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
};
use windows::Win32::Foundation::{HLOCAL, HWND, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
use windows::core::{HSTRING, Interface, factory, w};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use zeroize::Zeroizing;

use super::OwnerCheck;
use crate::FfiError;

const ENTROPY: &[u8] = b"aegis-vault-v1";

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, VaultError> {
    let len =
        u32::try_from(bytes.len()).map_err(|_| VaultError::Keystore("key too large".into()))?;
    Ok(CRYPT_INTEGER_BLOB {
        cbData: len,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

fn take_output(output: CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
    let mut bytes = Zeroizing::new(Vec::new());
    if !output.pbData.is_null() {
        let len = output.cbData as usize;
        // SAFETY: DPAPI returned a live allocation of cbData bytes and retains it until LocalFree.
        unsafe {
            bytes.extend_from_slice(std::slice::from_raw_parts(output.pbData, len));
        }
        // SAFETY: The DPAPI allocation is writable for cbData bytes and no references to it remain.
        unsafe {
            std::ptr::write_bytes(output.pbData, 0, len);
        }
        // SAFETY: DPAPI allocated this pointer with LocalAlloc and it is freed exactly once here.
        unsafe {
            LocalFree(Some(HLOCAL(output.pbData.cast())));
        }
    }
    bytes
}

pub(super) fn seal(plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let input = blob(plaintext)?;
    let entropy = blob(ENTROPY)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: The input and entropy blobs borrow live slices and output points to writable storage.
    unsafe {
        CryptProtectData(
            &input,
            w!("Aegis vault key"),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|error| VaultError::Keystore(error.to_string()))?;
    }
    Ok(take_output(output).to_vec())
}

pub(super) fn unseal(sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let input = blob(sealed)?;
    let entropy = blob(ENTROPY)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: The input and entropy blobs borrow live slices and output points to writable storage.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|error| VaultError::Keystore(error.to_string()))?;
    }
    Ok(take_output(output))
}

fn map_result(result: UserConsentVerificationResult) -> OwnerCheck {
    match result {
        UserConsentVerificationResult::Verified => OwnerCheck::Verified,
        UserConsentVerificationResult::DeviceNotPresent
        | UserConsentVerificationResult::NotConfiguredForUser
        | UserConsentVerificationResult::DisabledByPolicy => OwnerCheck::Unavailable,
        _ => OwnerCheck::Denied,
    }
}

fn request_for_window<T: Interface>(
    interop: &IUserConsentVerifierInterop,
    hwnd: HWND,
    reason: &HSTRING,
    _result_type: fn(&HSTRING) -> windows::core::Result<T>,
) -> windows::core::Result<T> {
    // SAFETY: The handle belongs to the live window borrowed by the caller.
    unsafe { interop.RequestVerificationForWindowAsync(hwnd, reason) }
}

pub(super) fn verify_owner(
    window: &impl HasWindowHandle,
    reason: &str,
    done: Box<dyn FnOnce(OwnerCheck) + Send>,
) -> Result<(), FfiError> {
    let handle = window
        .window_handle()
        .map_err(|error| FfiError::Native(error.to_string()))?;
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return Err(FfiError::Native("expected a Win32 window".into()));
    };
    let hwnd = HWND(raw.hwnd.get() as *mut _);
    let interop: IUserConsentVerifierInterop =
        factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
            .map_err(|error| FfiError::Native(error.to_string()))?;
    let operation = request_for_window(
        &interop,
        hwnd,
        &HSTRING::from(reason),
        UserConsentVerifier::RequestVerificationAsync,
    )
    .map_err(|error| FfiError::Native(error.to_string()))?;
    let pending = Arc::new(Mutex::new(Some(done)));
    let worker_pending = Arc::clone(&pending);
    let worker = std::thread::Builder::new()
        .name("aegis-owner-check".into())
        .spawn(move || {
            // SAFETY: This new worker thread has no COM apartment and is uninitialized.
            let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let result = if initialized {
                operation
                    .join()
                    .map(map_result)
                    .unwrap_or(OwnerCheck::Denied)
            } else {
                OwnerCheck::Denied
            };
            if initialized {
                // SAFETY: This worker thread successfully initialized its COM apartment above.
                unsafe { CoUninitialize() };
            }
            let done = worker_pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            if let Some(done) = done {
                done(result);
            }
        });
    if let Err(error) = worker {
        let done = pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(done) = done {
            done(OwnerCheck::Unavailable);
        }
        return Err(FfiError::Native(error.to_string()));
    }
    Ok(())
}

pub(super) fn owner_check_available() -> bool {
    UserConsentVerifier::CheckAvailabilityAsync()
        .and_then(|operation| operation.join())
        .is_ok_and(|result| result == UserConsentVerifierAvailability::Available)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trip() {
        let key = Zeroizing::new(vec![42; 32]);
        let sealed = seal(&key).unwrap();
        assert_ne!(sealed, *key);
        assert_eq!(*unseal(&sealed).unwrap(), *key);
    }

    #[test]
    fn owner_result_mapping() {
        assert_eq!(
            map_result(UserConsentVerificationResult::Verified),
            OwnerCheck::Verified
        );
        assert_eq!(
            map_result(UserConsentVerificationResult::DeviceNotPresent),
            OwnerCheck::Unavailable
        );
        assert_eq!(
            map_result(UserConsentVerificationResult::NotConfiguredForUser),
            OwnerCheck::Unavailable
        );
        assert_eq!(
            map_result(UserConsentVerificationResult::DisabledByPolicy),
            OwnerCheck::Unavailable
        );
        assert_eq!(
            map_result(UserConsentVerificationResult::Canceled),
            OwnerCheck::Denied
        );
    }
}
