use super::shortcut_keys::{Key, Mods, Platform, match_shortcut};
use super::{
    BlockRules, FfiError, NativeEvent, NativeFailReason, NativePermissionKind, RequestFilter,
    ResourceContext, Shortcut, SuspendOutcome, WindowMaterial,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, DownloadStartingEventHandler, FaviconChangedEventHandler,
    GetFaviconCompletedHandler, NavigationCompletedEventHandler, PermissionRequestedEventHandler,
    ProcessFailedEventHandler, TrySuspendCompletedHandler, WebResourceRequestedEventHandler,
    take_pwstr,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};
use windows::Win32::System::Com::IStream;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_F5, VK_L, VK_LEFT, VK_LWIN, VK_MENU, VK_OEM_COMMA, VK_R, VK_RIGHT,
    VK_RWIN, VK_S, VK_SHIFT, VK_T, VK_TAB, VK_W,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SystemParametersInfoW,
};
use windows::core::{HSTRING, Interface, PWSTR, w};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use wry::{WebView, WebViewExtWindows};

fn window_hwnd(window: &impl HasWindowHandle) -> Result<HWND, FfiError> {
    let handle = window
        .window_handle()
        .map_err(|error| FfiError::Native(error.to_string()))?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(FfiError::Native("expected a Win32 window".to_owned()));
    };
    Ok(HWND(handle.hwnd.get() as *mut c_void))
}

pub(super) fn window_glass_supported(window: &impl HasWindowHandle) -> bool {
    let Ok(hwnd) = window_hwnd(window) else {
        return false;
    };
    let mut backdrop = 0i32;
    // SAFETY: DWM writes one i32 to the valid output pointer for this live window.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&mut backdrop as *mut i32).cast(),
            size_of::<i32>() as u32,
        )
    }
    .is_ok()
}

pub(super) fn apply_window_material(
    window: &impl HasWindowHandle,
    material: WindowMaterial,
    dark: bool,
) -> Result<(), FfiError> {
    let hwnd = window_hwnd(window)?;
    if !window_glass_supported(window) {
        return Ok(());
    }
    let backdrop = match material {
        WindowMaterial::Glass => DWMSBT_TRANSIENTWINDOW.0,
        WindowMaterial::Solid => DWMSBT_NONE.0,
    };
    let corners = DWMWCP_ROUND.0;
    let dark = i32::from(dark);
    // SAFETY: each value is a live i32 with the size expected by its DWM attribute.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&backdrop as *const i32).cast(),
            size_of::<i32>() as u32,
        )
    }
    .map_err(native_error)?;
    // SAFETY: the corner preference is a defined DWM value for this live window.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corners as *const i32).cast(),
            size_of::<i32>() as u32,
        )
    }
    .map_err(native_error)?;
    // SAFETY: DWM reads the live dark-mode value synchronously for this window.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const i32).cast(),
            size_of::<i32>() as u32,
        )
    }
    .map_err(native_error)
}

pub(super) fn set_page_corner_radius(webview: &WebView, radius: f64) -> Result<(), FfiError> {
    if !radius.is_finite() || radius < 0.0 {
        return Err(FfiError::Native("invalid page corner radius".to_owned()));
    }
    let hwnd = webview.hwnd();
    if radius == 0.0 {
        // SAFETY: this is wry's live WebView2 host child HWND; a null region removes its clip.
        if unsafe { SetWindowRgn(hwnd, None, true) } == 0 {
            return Err(FfiError::Native("could not clear page region".to_owned()));
        }
        return Ok(());
    }
    let mut rect = RECT::default();
    // SAFETY: GetClientRect fills the initialized RECT for wry's live host child HWND.
    unsafe { GetClientRect(hwnd, &mut rect) }.map_err(native_error)?;
    // SAFETY: the HWND belongs to the live WebView2 host child window.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let radius = (radius * f64::from(dpi) / 96.0).clamp(0.0, f64::from(i32::MAX / 2));
    let diameter = (radius * 2.0).round() as i32;
    // SAFETY: these dimensions come from the child window's client rect.
    let region =
        unsafe { CreateRoundRectRgn(0, 0, rect.right + 1, rect.bottom + 1, diameter, diameter) };
    if region.is_invalid() {
        return Err(FfiError::Native("could not create page region".to_owned()));
    }
    // SAFETY: SetWindowRgn takes ownership of the region only when it succeeds.
    if unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
        // SAFETY: SetWindowRgn failed, so this process still owns the region.
        let _ = unsafe { DeleteObject(region.into()) };
        return Err(FfiError::Native("could not set page region".to_owned()));
    }
    Ok(())
}

pub(super) fn reduce_motion() -> bool {
    let mut enabled = 1i32;
    // SAFETY: SystemParametersInfoW writes one BOOL to the valid output pointer.
    let result = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut enabled as *mut i32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    result.is_ok() && enabled == 0
}

fn native_error(error: windows::core::Error) -> FfiError {
    FfiError::Native(error.to_string())
}

pub(super) fn lockdown(webview: &WebView) -> Result<(), FfiError> {
    let core = webview.webview();
    // SAFETY: wry returns a live WebView2 interface for this view.
    let settings = unsafe { core.Settings() }.map_err(native_error)?;
    // SAFETY: every setting call uses a live interface and a defined boolean value.
    unsafe { settings.SetIsWebMessageEnabled(false) }.map_err(native_error)?;
    // SAFETY: the live settings interface accepts this defined boolean value.
    unsafe { settings.SetAreHostObjectsAllowed(false) }.map_err(native_error)?;
    // SAFETY: the live settings interface accepts this defined boolean value.
    unsafe { settings.SetAreDevToolsEnabled(cfg!(debug_assertions)) }.map_err(native_error)?;
    // SAFETY: the live settings interface accepts this defined boolean value.
    unsafe { settings.SetIsStatusBarEnabled(false) }.map_err(native_error)?;
    let settings4: ICoreWebView2Settings4 = settings.cast().map_err(native_error)?;
    // SAFETY: the newer settings interface belongs to the same live webview.
    unsafe { settings4.SetIsGeneralAutofillEnabled(false) }.map_err(native_error)?;
    // SAFETY: the newer settings interface belongs to the same live webview.
    unsafe { settings4.SetIsPasswordAutosaveEnabled(false) }.map_err(native_error)?;
    let settings8: ICoreWebView2Settings8 = settings.cast().map_err(native_error)?;
    // smartscreen sends visited urls to microsoft; a later setting can opt in
    // SAFETY: the interface belongs to the live view and false is a valid setting.
    unsafe { settings8.SetIsReputationCheckingRequired(false) }.map_err(native_error)?;
    let core13: ICoreWebView2_13 = core.cast().map_err(native_error)?;
    // SAFETY: the profile is owned by this live webview.
    let profile = unsafe { core13.Profile() }.map_err(native_error)?;
    let profile3: ICoreWebView2Profile3 = profile.cast().map_err(native_error)?;
    // SAFETY: Strict is a defined tracking-prevention level for the live profile.
    unsafe {
        profile3.SetPreferredTrackingPreventionLevel(COREWEBVIEW2_TRACKING_PREVENTION_LEVEL_STRICT)
    }
    .map_err(native_error)
}

#[derive(Clone)]
pub(super) struct PreparedContentRules;

pub(super) fn prepare_content_rules(
    _json_lists: Vec<String>,
    on_ready: Box<dyn FnOnce(Result<(), FfiError>)>,
) -> Result<PreparedContentRules, FfiError> {
    on_ready(Ok(()));
    Ok(PreparedContentRules)
}

impl PreparedContentRules {
    pub(super) fn set_blocking(&self, _webview: &WebView, _enabled: bool) -> Result<(), FfiError> {
        Ok(())
    }
    pub(super) fn is_ready(&self) -> bool {
        true
    }

    pub(super) fn attach(
        &self,
        webview: &WebView,
        should_block: RequestFilter,
        on_blocked: Box<dyn Fn(&str)>,
    ) -> Result<(), FfiError> {
        install_request_filter(webview, should_block, on_blocked)
    }
}

struct PendingPermission {
    args: ICoreWebView2PermissionRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
    state: COREWEBVIEW2_PERMISSION_STATE,
}

impl Drop for PendingPermission {
    fn drop(&mut self) {
        // SAFETY: the retained arguments and deferral remain valid until completion.
        unsafe {
            let _ = self.args.SetState(self.state);
            let _ = self.deferral.Complete();
        }
    }
}

struct PendingDownload {
    args: ICoreWebView2DownloadStartingEventArgs,
    deferral: ICoreWebView2Deferral,
    name: String,
    cancel: bool,
}

impl Drop for PendingDownload {
    fn drop(&mut self) {
        // SAFETY: the retained arguments and deferral remain valid until completion.
        unsafe {
            let _ = self.args.SetCancel(self.cancel);
            let _ = self.deferral.Complete();
        }
    }
}

pub(super) struct NativeViewState {
    permissions: Rc<RefCell<HashMap<u64, PendingPermission>>>,
    downloads: Rc<RefCell<HashMap<u64, PendingDownload>>>,
}

impl Drop for NativeViewState {
    fn drop(&mut self) {
        self.permissions.borrow_mut().clear();
        self.downloads.borrow_mut().clear();
    }
}

impl NativeViewState {
    pub(super) fn answer_permission(
        &self,
        id: u64,
        allow: bool,
        remember: bool,
    ) -> Result<(), FfiError> {
        let Some(mut pending) = self.permissions.borrow_mut().remove(&id) else {
            return Err(FfiError::Native("unknown permission request".to_owned()));
        };
        let state = if allow {
            COREWEBVIEW2_PERMISSION_STATE_ALLOW
        } else {
            COREWEBVIEW2_PERMISSION_STATE_DENY
        };
        // SAFETY: the retained event arguments remain live while the deferral is pending.
        unsafe { pending.args.SetState(state) }.map_err(native_error)?;
        if let Ok(args3) = pending
            .args
            .cast::<ICoreWebView2PermissionRequestedEventArgs3>()
        {
            // SAFETY: the retained event arguments remain live while the deferral is pending.
            unsafe { args3.SetSavesInProfile(remember) }.map_err(native_error)?;
        }
        pending.state = state;
        // the pending value completes the deferral when it drops
        Ok(())
    }

    pub(super) fn answer_download(&self, id: u64, accept: bool) -> Result<(), FfiError> {
        let Some(mut pending) = self.downloads.borrow_mut().remove(&id) else {
            return Err(FfiError::Native("unknown download request".to_owned()));
        };
        if !accept {
            return Ok(());
        }
        let Some(home) = std::env::var_os("USERPROFILE") else {
            return Err(FfiError::Native("USERPROFILE is unavailable".to_owned()));
        };
        let path = PathBuf::from(home).join("Downloads").join(&pending.name);
        let path = HSTRING::from(path.to_string_lossy().as_ref());
        // SAFETY: the absolute path and event arguments remain live while the deferral is pending.
        unsafe {
            pending
                .args
                .SetResultFilePath(&path)
                .map_err(native_error)?;
            pending.args.SetCancel(false).map_err(native_error)?;
        }
        pending.cancel = false;
        Ok(())
    }
}

// canceled navigations (https upgrades, superseded loads) are not page failures
fn fail_reason(status: COREWEBVIEW2_WEB_ERROR_STATUS) -> NativeFailReason {
    match status {
        COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED => NativeFailReason::Canceled,
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_REVOKED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CLIENT_CERTIFICATE_CONTAINS_ERRORS => {
            NativeFailReason::CertificateError
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_CANNOT_CONNECT
        | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET
        | COREWEBVIEW2_WEB_ERROR_STATUS_DISCONNECTED
        | COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED
        | COREWEBVIEW2_WEB_ERROR_STATUS_SERVER_UNREACHABLE
        | COREWEBVIEW2_WEB_ERROR_STATUS_TIMEOUT => NativeFailReason::ConnectionFailed,
        _ => NativeFailReason::Other,
    }
}

pub(super) fn install_native_hooks(
    webview: &WebView,
    on_event: Box<dyn Fn(NativeEvent)>,
) -> Result<NativeViewState, FfiError> {
    lockdown(webview)?;
    let core = webview.webview();
    let events: Rc<dyn Fn(NativeEvent)> = Rc::from(on_event);
    let permissions = Rc::new(RefCell::new(HashMap::new()));
    let downloads = Rc::new(RefCell::new(HashMap::new()));
    let next_id = Rc::new(Cell::new(1u64));

    let navigation_events = events.clone();
    let handler = NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let mut success = windows::core::BOOL::default();
        // SAFETY: WebView2 supplies live event arguments during this callback.
        unsafe { args.IsSuccess(&mut success)? };
        if success.as_bool() {
            return Ok(());
        }
        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
        let mut uri = PWSTR::null();
        // SAFETY: WebView2 writes the status and COM-allocated source into valid outputs.
        unsafe {
            args.WebErrorStatus(&mut status)?;
            sender.Source(&mut uri)?;
        }
        let reason = fail_reason(status);
        let url = take_pwstr(uri);
        #[cfg(debug_assertions)]
        eprintln!(
            "nav failed: status={} url={url} reason={reason:?}",
            status.0
        );
        navigation_events(NativeEvent::NavigationFailed { url, reason });
        Ok(())
    }));
    let mut token = 0;
    // SAFETY: WebView2 retains the handler for this live view.
    unsafe { core.add_NavigationCompleted(&handler, &mut token) }.map_err(native_error)?;

    if let Ok(core15) = core.cast::<ICoreWebView2_15>() {
        let favicon_events = events.clone();
        let handler = FaviconChangedEventHandler::create(Box::new(move |sender, _| {
            let Some(sender) = sender else { return Ok(()) };
            let mut uri = PWSTR::null();
            // SAFETY: WebView2 writes the current source into this valid output.
            unsafe { sender.Source(&mut uri)? };
            let url = take_pwstr(uri);
            let Ok(source) = sender.cast::<ICoreWebView2_15>() else {
                return Ok(());
            };
            let completed_events = favicon_events.clone();
            let completed = GetFaviconCompletedHandler::create(Box::new(move |result, stream| {
                if result.is_err() {
                    return Ok(());
                }
                let Some(stream) = stream else { return Ok(()) };
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let mut read = 0u32;
                    // SAFETY: the stream is live for this callback and the writable buffer is valid for its stated length.
                    unsafe {
                        stream
                            .Read(
                                chunk.as_mut_ptr().cast(),
                                chunk.len() as u32,
                                Some(&mut read),
                            )
                            .ok()?
                    };
                    if read == 0 {
                        break;
                    }
                    if bytes.len() + read as usize > 1024 * 1024 {
                        return Ok(());
                    }
                    bytes.extend_from_slice(&chunk[..read as usize]);
                }
                completed_events(NativeEvent::FaviconChanged { url, bytes });
                Ok(())
            }));
            // SAFETY: the live WebView2 interface retains this completion handler until it returns the PNG stream.
            unsafe { source.GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &completed)? };
            Ok(())
        }));
        // SAFETY: WebView2 retains this event handler for the lifetime of the live view.
        unsafe { core15.add_FaviconChanged(&handler, &mut token) }.map_err(native_error)?;
    }

    let crash_events = events.clone();
    let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
        // SAFETY: WebView2 writes a defined process kind to this valid output.
        unsafe { args.ProcessFailedKind(&mut kind)? };
        if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
            || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
        {
            crash_events(NativeEvent::ContentCrashed);
        }
        Ok(())
    }));
    // SAFETY: WebView2 retains the handler for this live view.
    unsafe { core.add_ProcessFailed(&handler, &mut token) }.map_err(native_error)?;

    let permission_events = events.clone();
    let pending_permissions = permissions.clone();
    let permission_id = next_id.clone();
    let handler = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut uri = PWSTR::null();
        let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
        // SAFETY: WebView2 writes the COM string and kind into valid outputs.
        unsafe {
            args.Uri(&mut uri)?;
            args.PermissionKind(&mut kind)?;
        }
        let origin = take_pwstr(uri);
        let kind = match kind {
            COREWEBVIEW2_PERMISSION_KIND_CAMERA => NativePermissionKind::Camera,
            COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => NativePermissionKind::Microphone,
            COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => NativePermissionKind::Location,
            COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => NativePermissionKind::Notifications,
            COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => NativePermissionKind::ClipboardRead,
            _ => NativePermissionKind::Other,
        };
        // SAFETY: the event arguments remain valid while their deferral is retained.
        let deferral = unsafe { args.GetDeferral()? };
        // SAFETY: denial is the initial state until the app explicitly answers.
        unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)? };
        if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
            // SAFETY: the retained event arguments remain live while the deferral is pending.
            unsafe { args3.SetSavesInProfile(false)? };
        }
        let id = permission_id.get();
        permission_id.set(id.saturating_add(1));
        pending_permissions.borrow_mut().insert(
            id,
            PendingPermission {
                args: args.clone(),
                deferral,
                state: COREWEBVIEW2_PERMISSION_STATE_DENY,
            },
        );
        permission_events(NativeEvent::PermissionRequested { id, origin, kind });
        Ok(())
    }));
    // SAFETY: WebView2 retains the handler for this live view.
    unsafe { core.add_PermissionRequested(&handler, &mut token) }.map_err(native_error)?;

    let core4: ICoreWebView2_4 = core.cast().map_err(native_error)?;
    let download_events = events;
    let pending_downloads = downloads.clone();
    let download_id = next_id;
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        // SAFETY: the operation and deferral belong to this live download event.
        let operation = unsafe { args.DownloadOperation()? };
        // SAFETY: the event arguments are valid while this callback requests a deferral.
        let deferral = unsafe { args.GetDeferral()? };
        let mut uri = PWSTR::null();
        let mut path = PWSTR::null();
        // SAFETY: WebView2 writes COM-allocated strings into these valid outputs.
        unsafe {
            operation.Uri(&mut uri)?;
            args.ResultFilePath(&mut path)?;
        }
        let url = take_pwstr(uri);
        let original_path = take_pwstr(path);
        let name = Path::new(&original_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("download")
            .to_owned();
        // SAFETY: cancellation is the initial state until the app explicitly accepts.
        unsafe { args.SetCancel(true)? };
        let id = download_id.get();
        download_id.set(id.saturating_add(1));
        pending_downloads.borrow_mut().insert(
            id,
            PendingDownload {
                args: args.clone(),
                deferral,
                name: name.clone(),
                cancel: true,
            },
        );
        download_events(NativeEvent::DownloadRequested {
            id,
            url,
            suggested_name: name,
        });
        Ok(())
    }));
    // SAFETY: WebView2 retains the handler for this live view.
    unsafe { core4.add_DownloadStarting(&handler, &mut token) }.map_err(native_error)?;
    Ok(NativeViewState {
        permissions,
        downloads,
    })
}

pub(super) fn install_block_rules(
    webview: &WebView,
    rules: &BlockRules,
    on_blocked: Box<dyn Fn(&str)>,
) -> Result<(), FfiError> {
    let substrings = rules.url_substrings.clone();
    install_request_filter(
        webview,
        Box::new(move |uri, _context| {
            substrings
                .iter()
                .any(|substring| !substring.is_empty() && uri.contains(substring))
        }),
        on_blocked,
    )
}

fn install_request_filter(
    webview: &WebView,
    should_block: RequestFilter,
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
        // SAFETY: WebView2 owns the event arguments for this synchronous callback.
        let mut native_context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_OTHER;
        // SAFETY: WebView2 writes one resource context to this valid output pointer.
        unsafe { args.ResourceContext(&mut native_context)? };
        let context = match native_context {
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => ResourceContext::Document,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => ResourceContext::Script,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => ResourceContext::Stylesheet,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => ResourceContext::Image,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => ResourceContext::Font,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => ResourceContext::Media,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
            | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH
            | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE => ResourceContext::Xhr,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => ResourceContext::Websocket,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => ResourceContext::Ping,
            _ => ResourceContext::Other,
        };
        if !should_block(&uri, context) {
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
        // SAFETY: these key state reads occur synchronously on the UI thread.
        let win = unsafe { GetKeyState(VK_LWIN.0 as i32) < 0 || GetKeyState(VK_RWIN.0 as i32) < 0 };
        if win {
            return Ok(());
        }
        let mut key = 0;
        // SAFETY: WebView2 writes the virtual key to this valid output pointer.
        unsafe { args.VirtualKey(&mut key)? };
        let key = match key {
            key if key == u32::from(VK_L.0) => Key::L,
            key if key == u32::from(VK_OEM_COMMA.0) => Key::Comma,
            key if key == u32::from(VK_T.0) => Key::T,
            key if key == u32::from(VK_W.0) => Key::W,
            key if key == u32::from(VK_S.0) => Key::S,
            key if key == u32::from(VK_TAB.0) => Key::Tab,
            key if key == u32::from(VK_R.0) => Key::R,
            key if key == u32::from(VK_F5.0) => Key::F5,
            key if key == u32::from(VK_LEFT.0) => Key::Left,
            key if key == u32::from(VK_RIGHT.0) => Key::Right,
            _ => return Ok(()),
        };
        let Some(shortcut) = match_shortcut(
            key,
            Mods {
                ctrl,
                shift,
                alt,
                command: false,
            },
            Platform::Windows,
        ) else {
            return Ok(());
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

pub(super) fn go_back(webview: &WebView) -> Result<bool, FfiError> {
    let core = webview.webview();
    let mut can_go_back = windows::core::BOOL::default();
    // SAFETY: the interface belongs to this live webview and the output pointer is valid.
    unsafe { core.CanGoBack(&mut can_go_back) }.map_err(native_error)?;
    if !can_go_back.as_bool() {
        return Ok(false);
    }
    // SAFETY: the interface belongs to this live webview and history permits navigation.
    unsafe { core.GoBack() }.map_err(native_error)?;
    Ok(true)
}

pub(super) fn go_forward(webview: &WebView) -> Result<(), FfiError> {
    let core = webview.webview();
    let mut can_go_forward = windows::core::BOOL::default();
    // SAFETY: the interface belongs to this live webview and the output pointer is valid.
    unsafe { core.CanGoForward(&mut can_go_forward) }.map_err(native_error)?;
    if can_go_forward.as_bool() {
        // SAFETY: the interface belongs to this live webview and history permits navigation.
        unsafe { core.GoForward() }.map_err(native_error)?;
    }
    Ok(())
}

pub(super) fn focus(webview: &WebView) -> Result<(), FfiError> {
    // SAFETY: the controller belongs to this live webview and the reason is a defined enum value.
    unsafe {
        webview
            .controller()
            .MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)
    }
    .map_err(native_error)
}

#[cfg(test)]
mod fail_tests {
    use super::*;

    #[test]
    fn canceled_is_not_a_page_failure() {
        assert_eq!(
            fail_reason(COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED),
            NativeFailReason::Canceled
        );
        assert_eq!(
            fail_reason(COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED),
            NativeFailReason::CertificateError
        );
        assert_eq!(
            fail_reason(COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED),
            NativeFailReason::ConnectionFailed
        );
        assert_eq!(
            fail_reason(COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN),
            NativeFailReason::Other
        );
    }
}
