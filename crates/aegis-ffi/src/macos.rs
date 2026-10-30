use std::cell::RefCell;
use std::ptr;
use std::rc::Rc;

use super::shortcut_keys::{Key, Mods, Platform, match_shortcut};
use super::{BlockRules, FfiError, Shortcut, SuspendOutcome, content_rules};
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags};
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};
use wry::{WebView, WebViewExtMacOS};

type ShortcutTarget = (usize, Rc<dyn Fn(Shortcut)>);

thread_local! {
    static SHORTCUT_TARGET: RefCell<Option<ShortcutTarget>> = RefCell::new(None);
    static SHORTCUT_MONITOR: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
}

fn event_key(event: &NSEvent) -> Option<Key> {
    if event.keyCode() == 48 {
        return Some(Key::Tab);
    }
    let characters = event.charactersIgnoringModifiers()?;
    match characters.to_string().chars().next()?.to_ascii_lowercase() {
        'l' => Some(Key::L),
        't' => Some(Key::T),
        'w' => Some(Key::W),
        'r' => Some(Key::R),
        's' => Some(Key::S),
        '[' => Some(Key::LeftBracket),
        ']' => Some(Key::RightBracket),
        _ => None,
    }
}

fn event_shortcut(event: &NSEvent) -> Option<Shortcut> {
    let flags = event.modifierFlags()
        & NSEventModifierFlags::DeviceIndependentFlagsMask
        & !NSEventModifierFlags::CapsLock;
    let allowed = NSEventModifierFlags::Control
        | NSEventModifierFlags::Shift
        | NSEventModifierFlags::Option
        | NSEventModifierFlags::Command;
    if !(flags & !allowed).is_empty() {
        return None;
    }
    let mods = Mods {
        ctrl: flags.contains(NSEventModifierFlags::Control),
        shift: flags.contains(NSEventModifierFlags::Shift),
        alt: flags.contains(NSEventModifierFlags::Option),
        command: flags.contains(NSEventModifierFlags::Command),
    };
    match_shortcut(event_key(event)?, mods, Platform::MacOS)
}

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
    webview: &WebView,
    on_shortcut: Box<dyn Fn(Shortcut)>,
) -> Result<(), FfiError> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| FfiError::Native("shortcut monitor requires the main thread".to_owned()))?;
    let window = webview
        .webview()
        .window()
        .ok_or_else(|| FfiError::Native("webview has no window".to_owned()))?;
    let window_ptr = Retained::as_ptr(&window) as usize;
    SHORTCUT_MONITOR.with(|slot| -> Result<(), FfiError> {
        if slot.borrow().is_some() {
            return Ok(());
        }
        let block = RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| {
            // SAFETY: AppKit supplies a live event for the duration of this monitor callback.
            let native = unsafe { event.as_ref() };
            let Some(event_window) = native.window(mtm) else {
                return event.as_ptr();
            };
            let event_window_ptr = Retained::as_ptr(&event_window) as usize;
            let callback = SHORTCUT_TARGET.with(|target| {
                target.borrow().as_ref().and_then(|(window_ptr, callback)| {
                    (event_window_ptr == *window_ptr).then(|| Rc::clone(callback))
                })
            });
            if let (Some(callback), Some(shortcut)) = (callback, event_shortcut(native)) {
                callback(shortcut);
                return ptr::null_mut();
            }
            event.as_ptr()
        });
        // SAFETY: The block returns either AppKit's live event pointer or null to consume it.
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
        }
        .ok_or_else(|| FfiError::Native("shortcut monitor unavailable".to_owned()))?;
        *slot.borrow_mut() = Some(monitor);
        Ok(())
    })?;
    // all webviews use one app callback, so each install replaces the active window target
    SHORTCUT_TARGET.with(|target| {
        *target.borrow_mut() = Some((window_ptr, Rc::from(on_shortcut)));
    });
    Ok(())
}

pub(super) fn go_back(webview: &WebView) -> Result<bool, FfiError> {
    let native = webview.webview();
    // SAFETY: wry returns a live WKWebView subclass on the main thread.
    if !unsafe { native.canGoBack() } {
        return Ok(false);
    }
    // SAFETY: wry returns a live WKWebView subclass on the main thread.
    unsafe { native.goBack() };
    Ok(true)
}

pub(super) fn go_forward(webview: &WebView) -> Result<(), FfiError> {
    let native = webview.webview();
    // SAFETY: wry returns a live WKWebView subclass on the main thread.
    unsafe { native.goForward() };
    Ok(())
}

pub(super) fn focus(webview: &WebView) -> Result<(), FfiError> {
    let native = webview.webview();
    let window = native
        .window()
        .ok_or_else(|| FfiError::Native("webview has no window".to_owned()))?;
    if !window.makeFirstResponder(Some(&native)) {
        return Err(FfiError::Native("webview did not take focus".to_owned()));
    }
    Ok(())
}
