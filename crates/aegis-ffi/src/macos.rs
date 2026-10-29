use super::{BlockRules, FfiError, Shortcut, SuspendOutcome, content_rules};
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};
use wry::{WebView, WebViewExtMacOS};

pub(super) fn install_block_rules(
    webview: &WebView,
    rules: &BlockRules,
    _on_blocked: Box<dyn Fn(&str)>,
) -> Result<(), FfiError> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| FfiError::Native("content rules require the main thread".to_owned()))?;
    // SAFETY: the call runs on the main thread, as checked above.
    let store = unsafe { WKContentRuleListStore::defaultStore(mtm) }
        .ok_or_else(|| FfiError::Native("content rule store unavailable".to_owned()))?;
    let json = NSString::from_str(&content_rules::build_json(&rules.url_substrings));
    let identifier = NSString::from_str("aegis-spike-block-rules");
    let manager = webview.manager();
    // webkit content rules do not report individual blocked requests
    let completed = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
        if !error.is_null() {
            // SAFETY: WebKit passes a live error object for this callback.
            if let Some(error) = unsafe { error.as_ref() } {
                eprintln!(
                    "content rule compilation failed: {}",
                    error.localizedDescription()
                );
            }
            return;
        }
        // SAFETY: WebKit keeps the compiled list alive for the duration of this callback.
        if let Some(list) = unsafe { list.as_ref() } {
            // SAFETY: the manager and list are live Objective-C objects on the main thread.
            unsafe { manager.addContentRuleList(list) };
        }
    });
    // m5 must compile rules before creating webviews so the first load is covered
    // SAFETY: all arguments are non-null and WebKit copies the completion block.
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&identifier),
            Some(&json),
            Some(&completed),
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
    // the app menu owns macOS shortcuts in M4
    Ok(())
}
