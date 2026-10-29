use super::{BlockRules, FfiError, Shortcut, SuspendOutcome};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, TrySuspendCompletedHandler,
    WebResourceRequestedEventHandler, take_pwstr,
};
use windows::Win32::System::Com::IStream;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_L, VK_MENU, VK_SHIFT, VK_T,
};
use windows::core::{Interface, PWSTR, w};
use wry::{WebView, WebViewExtWindows};

fn native_error(error: windows::core::Error) -> FfiError {
    FfiError::Native(error.to_string())
}

pub(super) fn install_block_rules(
    webview: &WebView,
    rules: &BlockRules,
    on_blocked: Box<dyn Fn(&str)>,
) -> Result<(), FfiError> {
    let core = webview.webview();
    let environment = webview.environment();

    if let Ok(new_core) = core.cast::<ICoreWebView2_22>() {
        // SAFETY: the filter pattern is a static null-terminated string and the interface is valid.
        unsafe {
            new_core.AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )
        }
        .map_err(native_error)?;
    } else {
        // SAFETY: the filter pattern is a static null-terminated string and the interface is valid.
        unsafe {
            core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
        }
        .map_err(native_error)?;
    }

    let substrings = rules.url_substrings.clone();
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        // SAFETY: WebView2 owns the event arguments for this synchronous callback.
        let request = unsafe { args.Request()? };
        let mut uri = PWSTR::null();
        // SAFETY: WebView2 writes a COM-allocated string to this valid output pointer.
        unsafe { request.Uri(&mut uri)? };
        let uri = take_pwstr(uri);
        if !substrings
            .iter()
            .any(|substring| !substring.is_empty() && uri.contains(substring))
        {
            return Ok(());
        }
        // SAFETY: the environment is alive with the webview and the static strings are null-terminated.
        let response = unsafe {
            environment.CreateWebResourceResponse(None::<&IStream>, 403, w!("Forbidden"), w!(""))?
        };
        // SAFETY: the response is valid and the event arguments are live for this callback.
        unsafe { args.SetResponse(&response)? };
        on_blocked(&uri);
        Ok(())
    }));
    // the webview owns this registration for its lifetime
    let mut _token = 0;
    // SAFETY: WebView2 retains the handler until the webview is destroyed.
    unsafe { core.add_WebResourceRequested(&handler, &mut _token) }.map_err(native_error)?;
    Ok(())
}

pub(super) fn suspend(
    webview: &WebView,
    done: Box<dyn FnOnce(SuspendOutcome)>,
) -> Result<(), FfiError> {
    let controller = webview.controller();
    // webview2 won't suspend a visible webview, hide it first
    // SAFETY: the controller belongs to this live webview.
    unsafe { controller.SetIsVisible(false) }.map_err(native_error)?;
    let core = webview.webview();
    let core: ICoreWebView2_3 = match core.cast() {
        Ok(core) => core,
        Err(error) => {
            // SAFETY: the controller belongs to this live webview.
            let _ = unsafe { controller.SetIsVisible(true) };
            return Err(native_error(error));
        }
    };
    let restore_controller = controller.clone();
    let handler = TrySuspendCompletedHandler::create(Box::new(move |result, suspended| {
        let outcome = if result.is_ok() && suspended {
            SuspendOutcome::Suspended
        } else {
            // SAFETY: the controller remains live until this completion callback returns.
            let _ = unsafe { restore_controller.SetIsVisible(true) };
            SuspendOutcome::Failed
        };
        done(outcome);
        Ok(())
    }));
    // SAFETY: the completion handler is owned by WebView2 until it fires.
    if let Err(error) = unsafe { core.TrySuspend(&handler) } {
        // SAFETY: the controller belongs to this live webview.
        let _ = unsafe { controller.SetIsVisible(true) };
        return Err(native_error(error));
    }
    Ok(())
}

pub(super) fn resume(webview: &WebView) -> Result<(), FfiError> {
    let core = webview.webview();
    let core: ICoreWebView2_3 = core.cast().map_err(native_error)?;
    // SAFETY: the interface belongs to this live webview.
    unsafe { core.Resume() }.map_err(native_error)?;
    // SAFETY: the controller belongs to this live webview.
    unsafe { webview.controller().SetIsVisible(true) }.map_err(native_error)
}

pub(super) fn set_background_memory(webview: &WebView, low: bool) -> Result<(), FfiError> {
    let core = webview.webview();
    let core: ICoreWebView2_19 = core.cast().map_err(native_error)?;
    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    // SAFETY: the interface belongs to this live webview and the level is a defined enum value.
    unsafe { core.SetMemoryUsageTargetLevel(level) }.map_err(native_error)
}

pub(super) fn install_shortcuts(
    webview: &WebView,
    on_shortcut: Box<dyn Fn(Shortcut)>,
) -> Result<(), FfiError> {
    let controller = webview.controller();
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
        // SAFETY: WebView2 writes the key kind to this valid output pointer.
        unsafe { args.KeyEventKind(&mut kind)? };
        if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN {
            return Ok(());
        }
        // SAFETY: these key state reads occur synchronously on the UI thread.
        let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
        // SAFETY: these key state reads occur synchronously on the UI thread.
        let alt = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;
        // SAFETY: these key state reads occur synchronously on the UI thread.
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        if !ctrl || alt || shift {
            return Ok(());
        }
        let mut key = 0;
        // SAFETY: WebView2 writes the virtual key to this valid output pointer.
        unsafe { args.VirtualKey(&mut key)? };
        let shortcut = match key {
            value if value == u32::from(VK_L.0) => Shortcut::CommandBar,
            value if value == u32::from(VK_T.0) => Shortcut::NewTab,
            _ => return Ok(()),
        };
        // SAFETY: the event arguments are live for this synchronous callback.
        unsafe { args.SetHandled(true)? };
        on_shortcut(shortcut);
        Ok(())
    }));
    // the controller owns this registration for its lifetime
    let mut _token = 0;
    // SAFETY: WebView2 retains the handler until the controller is destroyed.
    unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut _token) }.map_err(native_error)?;
    Ok(())
}
