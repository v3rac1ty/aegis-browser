use std::cell::RefCell;
use std::ptr;
use std::rc::Rc;

use super::shortcut_keys::{Key, Mods, Platform, match_shortcut};
use super::{
    BlockRules, FfiError, NativeEvent, Shortcut, SuspendOutcome, WindowMaterial, content_rules,
};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSEvent, NSEventMask, NSEventModifierFlags, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWorkspace,
};
use objc2_foundation::{NSError, NSString};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use wry::{WebView, WebViewExtMacOS};

type ShortcutTarget = (usize, Rc<dyn Fn(Shortcut)>);

pub(super) struct NativeViewState;

impl NativeViewState {
    pub(super) fn answer_permission(
        &self,
        _id: u64,
        _allow: bool,
        _remember: bool,
    ) -> Result<(), FfiError> {
        Err(FfiError::Native(
            "WKWebView permission decisions are not exposed".to_owned(),
        ))
    }

    pub(super) fn answer_download(&self, _id: u64, _accept: bool) -> Result<(), FfiError> {
        Err(FfiError::Native(
            "deferred downloads are unavailable on WKWebView".to_owned(),
        ))
    }
}

pub(super) fn install_native_hooks(
    _webview: &WebView,
    _on_event: Box<dyn Fn(NativeEvent)>,
) -> Result<NativeViewState, FfiError> {
    Ok(NativeViewState)
}

#[derive(Clone)]
pub(super) struct PreparedContentRules {
    list: Rc<RefCell<Option<Retained<WKContentRuleList>>>>,
}

impl PreparedContentRules {
    pub(super) fn set_blocking(&self, webview: &WebView, enabled: bool) -> Result<(), FfiError> {
        let list = self.list.borrow();
        let list = list
            .as_ref()
            .ok_or_else(|| FfiError::Native("content rules are not ready".to_owned()))?;
        let manager = webview.manager();
        // SAFETY: the manager and compiled rule list are live on the main thread.
        unsafe {
            if enabled {
                manager.addContentRuleList(list);
            } else {
                manager.removeContentRuleList(list);
            }
        }
        Ok(())
    }
    pub(super) fn is_ready(&self) -> bool {
        self.list.borrow().is_some()
    }

    pub(super) fn attach(
        &self,
        webview: &WebView,
        _should_block: super::RequestFilter,
        _on_blocked: Box<dyn Fn(&str)>,
    ) -> Result<(), FfiError> {
        let list = self.list.borrow();
        let list = list
            .as_ref()
            .ok_or_else(|| FfiError::Native("content rules are not ready".to_owned()))?;
        let manager = webview.manager();
        // SAFETY: the compiled rule list and manager are live on the main thread.
        unsafe { manager.addContentRuleList(list) };
        Ok(())
    }
}

pub(super) fn prepare_content_rules(
    json_lists: Vec<String>,
    on_ready: Box<dyn FnOnce(Result<(), FfiError>)>,
) -> Result<PreparedContentRules, FfiError> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| FfiError::Native("content rules require the main thread".to_owned()))?;
    // SAFETY: the store is requested on the main thread as required by WebKit.
    let store = unsafe { WKContentRuleListStore::defaultStore(mtm) }
        .ok_or_else(|| FfiError::Native("content rule store unavailable".to_owned()))?;
    let list = Rc::new(RefCell::new(None));
    let result = list.clone();
    let done = RefCell::new(Some(on_ready));
    let json = NSString::from_str(&content_rules::merge_json_lists(json_lists));
    let identifier = NSString::from_str("aegis-content-rules");
    let completed = RcBlock::new(
        move |compiled: *mut WKContentRuleList, error: *mut NSError| {
            let outcome = if !error.is_null() {
                // SAFETY: WebKit passes a live error object for this callback.
                let message = unsafe { error.as_ref() }
                    .map(|error| error.localizedDescription().to_string())
                    .unwrap_or_else(|| "content rule compilation failed".to_owned());
                Err(FfiError::Native(message))
            } else {
                // SAFETY: a non-null compiled list is retained before the callback ends.
                match unsafe { Retained::retain(compiled) } {
                    Some(compiled) => {
                        *result.borrow_mut() = Some(compiled);
                        Ok(())
                    }
                    None => Err(FfiError::Native(
                        "content rule compilation returned no list".to_owned(),
                    )),
                }
            };
            if let Some(on_ready) = done.borrow_mut().take() {
                on_ready(outcome);
            }
        },
    );
    // SAFETY: the store and strings are live and WebKit copies the completion block.
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&identifier),
            Some(&json),
            Some(&completed),
        );
    }
    Ok(PreparedContentRules { list })
}

thread_local! {
    static SHORTCUT_TARGET: RefCell<Option<ShortcutTarget>> = const { RefCell::new(None) };
    static SHORTCUT_MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static MATERIAL_VIEW: RefCell<Option<(usize, Retained<NSVisualEffectView>)>> = const { RefCell::new(None) };
}

fn window_view(window: &impl HasWindowHandle) -> Result<&NSView, FfiError> {
    let handle = window
        .window_handle()
        .map_err(|error| FfiError::Native(error.to_string()))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(FfiError::Native("expected an AppKit window".to_owned()));
    };
    // SAFETY: AppKitWindowHandle keeps this NSView live while the window handle is borrowed.
    Ok(unsafe { &*(handle.ns_view.as_ptr() as *const NSView) })
}

pub(super) fn window_glass_supported(_window: &impl HasWindowHandle) -> bool {
    true
}

pub(super) fn apply_window_material(
    window: &impl HasWindowHandle,
    material: WindowMaterial,
    _dark: bool,
) -> Result<(), FfiError> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| FfiError::Native("window material needs the main thread".to_owned()))?;
    let native_view = window_view(window)?;
    let native_window = native_view
        .window()
        .ok_or_else(|| FfiError::Native("window has no AppKit content".to_owned()))?;
    let key = Retained::as_ptr(&native_window) as usize;
    MATERIAL_VIEW.with(|slot| {
        if let Some((existing, effect)) = slot.borrow().as_ref()
            && *existing == key
        {
            effect.setHidden(material == WindowMaterial::Solid);
            return Ok(());
        }
        let content = native_window
            .contentView()
            .ok_or_else(|| FfiError::Native("window has no content view".to_owned()))?;
        let wrapper = NSView::new(mtm);
        wrapper.setFrame(content.frame());
        let sizing = NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable;
        content.setFrame(wrapper.bounds());
        content.setAutoresizingMask(sizing);
        let effect = NSVisualEffectView::new(mtm);
        effect.setFrame(wrapper.bounds());
        effect.setAutoresizingMask(sizing);
        effect.setMaterial(NSVisualEffectMaterial::Sidebar);
        effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        effect.setState(NSVisualEffectState::Active);
        effect.setHidden(material == WindowMaterial::Solid);
        wrapper.addSubview(&effect);
        wrapper.addSubview(&content);
        native_window.setContentView(Some(&wrapper));
        *slot.borrow_mut() = Some((key, effect));
        Ok(())
    })
}

pub(super) fn set_page_corner_radius(webview: &WebView, radius: f64) -> Result<(), FfiError> {
    if !radius.is_finite() || radius < 0.0 {
        return Err(FfiError::Native("invalid page corner radius".to_owned()));
    }
    let native = webview.webview();
    native.setWantsLayer(true);
    // SAFETY: the WKWebView retains its layer for the lifetime of this synchronous call.
    let layer: *mut AnyObject = unsafe { msg_send![&native, layer] };
    if layer.is_null() {
        return Err(FfiError::Native("webview has no layer".to_owned()));
    }
    // SAFETY: the non-null layer belongs to the live WKWebView and accepts a CGFloat radius.
    let (): () = unsafe { msg_send![layer, setCornerRadius: radius] };
    // SAFETY: the non-null layer belongs to the live WKWebView and accepts a BOOL mask flag.
    let (): () = unsafe { msg_send![layer, setMasksToBounds: radius > 0.0] };
    Ok(())
}

pub(super) fn reduce_motion() -> bool {
    if MainThreadMarker::new().is_none() {
        return false;
    }
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
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
        ',' => Some(Key::Comma),
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
