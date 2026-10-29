use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::ptr;

use super::{BlockRules, FfiError, Shortcut, SuspendOutcome, content_rules};
use glib::translate::ToGlibPtr;
use webkit2gtk::{UserContentManager, WebViewExt};
use wry::{WebView, WebViewExtUnix};

struct SaveContext {
    manager: UserContentManager,
    store: *mut webkit2gtk_sys::WebKitUserContentFilterStore,
    bytes: *mut glib_sys::GBytes,
}

unsafe extern "C" fn save_finished(
    _source: *mut glib::gobject_ffi::GObject,
    result: *mut gio_sys::GAsyncResult,
    user_data: glib_sys::gpointer,
) {
    // SAFETY: save passes this Box pointer once to this completion callback.
    let context = unsafe { Box::from_raw(user_data.cast::<SaveContext>()) };
    let mut error = ptr::null_mut();
    // SAFETY: the store remains owned by the context and WebKit supplies the live result.
    let filter = unsafe {
        webkit2gtk_sys::webkit_user_content_filter_store_save_finish(
            context.store,
            result,
            &mut error,
        )
    };
    if !error.is_null() {
        // SAFETY: WebKit returns an allocated GError whose message is a null-terminated string.
        unsafe {
            eprintln!(
                "content filter save failed: {}",
                CStr::from_ptr((*error).message).to_string_lossy()
            );
            glib_sys::g_error_free(error);
        }
    }
    if !filter.is_null() && error.is_null() {
        // SAFETY: the context owns the manager and the returned filter until it is unrefed here.
        unsafe {
            webkit2gtk_sys::webkit_user_content_manager_add_filter(
                context.manager.to_glib_none().0,
                filter,
            );
            webkit2gtk_sys::webkit_user_content_filter_unref(filter);
        }
    } else if !filter.is_null() {
        // SAFETY: WebKit returned an owned filter even though it also reported an error.
        unsafe { webkit2gtk_sys::webkit_user_content_filter_unref(filter) };
    } else if error.is_null() {
        eprintln!("content filter save failed without an error");
    }
    // SAFETY: the context owns one reference to both objects until this callback finishes.
    unsafe {
        glib_sys::g_bytes_unref(context.bytes);
        glib::gobject_ffi::g_object_unref(context.store.cast());
    }
}

pub(super) fn install_block_rules(
    webview: &WebView,
    rules: &BlockRules,
    _on_blocked: Box<dyn Fn(&str)>,
) -> Result<(), FfiError> {
    let manager = webview
        .webview()
        .user_content_manager()
        .ok_or_else(|| FfiError::Native("user content manager unavailable".to_owned()))?;
    let directory = std::env::temp_dir().join("aegis-content-filters");
    std::fs::create_dir_all(&directory).map_err(|error| FfiError::Native(error.to_string()))?;
    let path = CString::new(directory.as_os_str().as_bytes())
        .map_err(|error| FfiError::Native(error.to_string()))?;
    let json = content_rules::build_json(&rules.url_substrings);

    // SAFETY: the path is a live null-terminated string for the duration of this call.
    let store = unsafe { webkit2gtk_sys::webkit_user_content_filter_store_new(path.as_ptr()) };
    if store.is_null() {
        return Err(FfiError::Native(
            "content filter store unavailable".to_owned(),
        ));
    }
    // SAFETY: g_bytes_new copies the live JSON bytes into a new owned GBytes.
    let bytes = unsafe { glib_sys::g_bytes_new(json.as_ptr().cast(), json.len()) };
    if bytes.is_null() {
        // SAFETY: the store was returned with one owned reference above.
        unsafe { glib::gobject_ffi::g_object_unref(store.cast()) };
        return Err(FfiError::Native(
            "content filter bytes unavailable".to_owned(),
        ));
    }
    let context = Box::new(SaveContext {
        manager,
        store,
        bytes,
    });
    // SAFETY: the callback takes the context exactly once, and its owned objects stay live until then.
    unsafe {
        webkit2gtk_sys::webkit_user_content_filter_store_save(
            store,
            c"aegis-spike-block-rules".as_ptr(),
            bytes,
            ptr::null_mut(),
            Some(save_finished),
            Box::into_raw(context).cast(),
        );
    }
    Ok(())
}

pub(super) fn suspend(
    _webview: &WebView,
    done: Box<dyn FnOnce(SuspendOutcome)>,
) -> Result<(), FfiError> {
    done(SuspendOutcome::Unsupported);
    Ok(())
}

pub(super) fn resume(_webview: &WebView) -> Result<(), FfiError> {
    Ok(())
}

pub(super) fn set_background_memory(_webview: &WebView, _low: bool) -> Result<(), FfiError> {
    Ok(())
}

pub(super) fn install_shortcuts(
    _webview: &WebView,
    _on_shortcut: Box<dyn Fn(Shortcut)>,
) -> Result<(), FfiError> {
    // the app menu owns Linux shortcuts in M4
    Ok(())
}
