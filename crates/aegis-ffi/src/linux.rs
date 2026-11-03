use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::ptr;
use std::rc::Rc;

use super::shortcut_keys::{Key, Mods, Platform, match_shortcut};
use super::{
    BlockRules, FfiError, NativeEvent, NativeFailReason, NativePermissionKind, Shortcut,
    SuspendOutcome, content_rules,
};
use glib::prelude::{Cast, ObjectExt};
use glib::translate::ToGlibPtr;
use gtk::cairo::{Format, ImageSurface};
use gtk::gdk;
use gtk::prelude::WidgetExt;
use webkit2gtk::{
    CookieAcceptPolicy, CookieManagerExt, GeolocationPermissionRequest, HardwareAccelerationPolicy,
    NetworkError, NotificationPermissionRequest, PermissionRequest, PermissionRequestExt,
    UserContentManager, UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebContextExt,
    WebViewExt, WebsiteDataManagerExt,
};
use wry::{WebView, WebViewExtUnix};

pub(super) struct NativeViewState {
    permissions: Rc<RefCell<HashMap<u64, PermissionRequest>>>,
}

impl Drop for NativeViewState {
    fn drop(&mut self) {
        for request in self
            .permissions
            .borrow_mut()
            .drain()
            .map(|(_, request)| request)
        {
            request.deny();
        }
    }
}

impl NativeViewState {
    pub(super) fn answer_permission(
        &self,
        id: u64,
        allow: bool,
        _remember: bool,
    ) -> Result<(), FfiError> {
        let request = self
            .permissions
            .borrow_mut()
            .remove(&id)
            .ok_or_else(|| FfiError::Native("unknown permission request".to_owned()))?;
        if allow {
            request.allow();
        } else {
            request.deny();
        }
        Ok(())
    }

    pub(super) fn answer_download(&self, _id: u64, _accept: bool) -> Result<(), FfiError> {
        Err(FfiError::Native(
            "deferred downloads are unavailable on WebKitGTK".to_owned(),
        ))
    }
}

pub(super) fn install_native_hooks(
    webview: &WebView,
    on_event: Box<dyn Fn(NativeEvent)>,
) -> Result<NativeViewState, FfiError> {
    let native = webview.webview();
    let context = native
        .context()
        .ok_or_else(|| FfiError::Native("WebKit context unavailable".to_owned()))?;
    if context.favicon_database_directory().is_none()
        && let Some(data_dir) = context
            .website_data_manager()
            .and_then(|manager| manager.base_data_directory())
    {
        let path = std::path::Path::new(data_dir.as_str()).join("favicons");
        context.set_favicon_database_directory(Some(&path.to_string_lossy()));
    }
    let cookies = context
        .cookie_manager()
        .ok_or_else(|| FfiError::Native("cookie manager unavailable".to_owned()))?;
    cookies.set_accept_policy(CookieAcceptPolicy::NoThirdParty);
    let on_event: Rc<dyn Fn(NativeEvent)> = Rc::from(on_event);
    let favicon_events = on_event.clone();
    native.connect_favicon_notify(move |view| {
        let url = view.uri().map(|uri| uri.to_string()).unwrap_or_default();
        let Some(surface) = view.favicon() else {
            return;
        };
        let Ok(image) = ImageSurface::try_from(surface) else {
            return;
        };
        if image.format() != Format::ARgb32 && image.format() != Format::Rgb24 {
            return;
        }
        let (width, height, stride) = (image.width(), image.height(), image.stride());
        if width <= 0 || height <= 0 || width > 512 || height > 512 || stride < width * 4 {
            return;
        }
        let opaque = image.format() == Format::Rgb24;
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        if image
            .with_data(|data| {
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let at = y * stride as usize + x * 4;
                        let alpha = if opaque { 255 } else { data[at + 3] };
                        for channel in [2, 1, 0] {
                            let premultiplied = u32::from(data[at + channel]);
                            let straight = if alpha == 0 {
                                0
                            } else {
                                ((premultiplied * 255 + u32::from(alpha) / 2) / u32::from(alpha))
                                    .min(255) as u8
                            };
                            rgba.push(straight);
                        }
                        rgba.push(alpha);
                    }
                }
            })
            .is_err()
        {
            return;
        }
        favicon_events(NativeEvent::FaviconRgba {
            url,
            rgba,
            width: width as u32,
            height: height as u32,
        });
    });
    let failed = on_event.clone();
    native.connect_load_failed(move |_, _, url, error| {
        let message = error.message().to_ascii_lowercase();
        let reason = if matches!(error.kind::<NetworkError>(), Some(NetworkError::Cancelled)) {
            NativeFailReason::Canceled
        } else if message.contains("certificate") || message.contains("tls") {
            NativeFailReason::CertificateError
        } else if matches!(
            error.kind::<NetworkError>(),
            Some(NetworkError::Failed | NetworkError::Transport)
        ) || message.contains("connection")
            || message.contains("resolve")
            || message.contains("network")
        {
            NativeFailReason::ConnectionFailed
        } else {
            NativeFailReason::Other
        };
        failed(NativeEvent::NavigationFailed {
            url: url.to_owned(),
            reason,
        });
        false
    });
    let crashed = on_event.clone();
    native.connect_web_process_terminated(move |_, _| crashed(NativeEvent::ContentCrashed));
    let permissions = Rc::new(RefCell::new(HashMap::new()));
    let pending = permissions.clone();
    let next_id = Rc::new(Cell::new(1u64));
    native.connect_permission_request(move |webview, request| {
        let kind = if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
            if media.is_for_video_device() {
                NativePermissionKind::Camera
            } else if media.is_for_audio_device() {
                NativePermissionKind::Microphone
            } else {
                NativePermissionKind::Other
            }
        } else if request.is::<GeolocationPermissionRequest>() {
            NativePermissionKind::Location
        } else if request.is::<NotificationPermissionRequest>() {
            NativePermissionKind::Notifications
        } else {
            NativePermissionKind::Other
        };
        let id = next_id.get();
        next_id.set(id.saturating_add(1));
        // webkitgtk's safe signal does not expose the requesting frame, so use the top-level site as in gnome web
        let origin = webview.uri().map(|uri| uri.to_string()).unwrap_or_default();
        pending.borrow_mut().insert(id, request.clone());
        on_event(NativeEvent::PermissionRequested { id, origin, kind });
        true
    });
    Ok(NativeViewState { permissions })
}

struct SaveContext {
    manager: UserContentManager,
    store: *mut webkit2gtk_sys::WebKitUserContentFilterStore,
    bytes: *mut glib_sys::GBytes,
}

struct Filter(*mut webkit2gtk_sys::WebKitUserContentFilter);

impl Drop for Filter {
    fn drop(&mut self) {
        // SAFETY: this filter owns the reference returned by the save completion call.
        unsafe { webkit2gtk_sys::webkit_user_content_filter_unref(self.0) };
    }
}

#[derive(Clone)]
pub(super) struct PreparedContentRules {
    filter: Rc<RefCell<Option<Filter>>>,
}

impl PreparedContentRules {
    pub(super) fn set_blocking(&self, webview: &WebView, enabled: bool) -> Result<(), FfiError> {
        let manager = webview
            .webview()
            .user_content_manager()
            .ok_or_else(|| FfiError::Native("user content manager unavailable".to_owned()))?;
        let filter = self.filter.borrow();
        let filter = filter
            .as_ref()
            .ok_or_else(|| FfiError::Native("content rules are not ready".to_owned()))?;
        // SAFETY: the manager and compiled filter remain live during this call.
        unsafe {
            if enabled {
                webkit2gtk_sys::webkit_user_content_manager_add_filter(
                    manager.to_glib_none().0,
                    filter.0,
                );
            } else {
                webkit2gtk_sys::webkit_user_content_manager_remove_all_filters(
                    manager.to_glib_none().0,
                );
            }
        }
        Ok(())
    }
    pub(super) fn is_ready(&self) -> bool {
        self.filter.borrow().is_some()
    }

    pub(super) fn attach(
        &self,
        webview: &WebView,
        _should_block: super::RequestFilter,
        _on_blocked: Box<dyn Fn(&str)>,
    ) -> Result<(), FfiError> {
        let manager = webview
            .webview()
            .user_content_manager()
            .ok_or_else(|| FfiError::Native("user content manager unavailable".to_owned()))?;
        let filter = self.filter.borrow();
        let filter = filter
            .as_ref()
            .ok_or_else(|| FfiError::Native("content rules are not ready".to_owned()))?;
        // SAFETY: the manager and compiled filter remain live during this call.
        unsafe {
            webkit2gtk_sys::webkit_user_content_manager_add_filter(
                manager.to_glib_none().0,
                filter.0,
            )
        };
        Ok(())
    }
}

struct PrepareContext {
    filter: Rc<RefCell<Option<Filter>>>,
    store: *mut webkit2gtk_sys::WebKitUserContentFilterStore,
    bytes: *mut glib_sys::GBytes,
    on_ready: Box<dyn FnOnce(Result<(), FfiError>)>,
}

unsafe extern "C" fn prepare_finished(
    _source: *mut glib::gobject_ffi::GObject,
    result: *mut gio_sys::GAsyncResult,
    user_data: glib_sys::gpointer,
) {
    // SAFETY: save passes this Box pointer once to this completion callback.
    let context = unsafe { Box::from_raw(user_data.cast::<PrepareContext>()) };
    let mut error = ptr::null_mut();
    // SAFETY: the store remains owned by the context and WebKit supplies a live result.
    let filter = unsafe {
        webkit2gtk_sys::webkit_user_content_filter_store_save_finish(
            context.store,
            result,
            &mut error,
        )
    };
    let outcome = if !error.is_null() {
        // SAFETY: WebKit returns an allocated error with a null-terminated message.
        let message = unsafe {
            CStr::from_ptr((*error).message)
                .to_string_lossy()
                .into_owned()
        };
        // SAFETY: the allocated GError is owned by this completion callback.
        unsafe { glib_sys::g_error_free(error) };
        if !filter.is_null() {
            // SAFETY: the returned filter reference is owned by this callback on failure.
            unsafe { webkit2gtk_sys::webkit_user_content_filter_unref(filter) };
        }
        Err(FfiError::Native(message))
    } else if filter.is_null() {
        Err(FfiError::Native(
            "content rule compilation returned no filter".to_owned(),
        ))
    } else {
        *context.filter.borrow_mut() = Some(Filter(filter));
        Ok(())
    };
    // SAFETY: these references were retained until the async operation completed.
    unsafe {
        glib_sys::g_bytes_unref(context.bytes);
        glib::gobject_ffi::g_object_unref(context.store.cast());
    }
    (context.on_ready)(outcome);
}

pub(super) fn prepare_content_rules(
    json_lists: Vec<String>,
    on_ready: Box<dyn FnOnce(Result<(), FfiError>)>,
) -> Result<PreparedContentRules, FfiError> {
    let directory = std::env::temp_dir().join("aegis-content-filters");
    std::fs::create_dir_all(&directory).map_err(|error| FfiError::Native(error.to_string()))?;
    let path = CString::new(directory.as_os_str().as_bytes())
        .map_err(|error| FfiError::Native(error.to_string()))?;
    let json = content_rules::merge_json_lists(json_lists);
    // SAFETY: the path remains valid for the duration of this constructor call.
    let store = unsafe { webkit2gtk_sys::webkit_user_content_filter_store_new(path.as_ptr()) };
    if store.is_null() {
        return Err(FfiError::Native(
            "content filter store unavailable".to_owned(),
        ));
    }
    // SAFETY: GBytes copies the live JSON bytes before this call returns.
    let bytes = unsafe { glib_sys::g_bytes_new(json.as_ptr().cast(), json.len()) };
    if bytes.is_null() {
        // SAFETY: the store owns one reference from its constructor.
        unsafe { glib::gobject_ffi::g_object_unref(store.cast()) };
        return Err(FfiError::Native(
            "content filter bytes unavailable".to_owned(),
        ));
    }
    let filter = Rc::new(RefCell::new(None));
    let context = Box::new(PrepareContext {
        filter: filter.clone(),
        store,
        bytes,
        on_ready,
    });
    // SAFETY: the callback takes the context exactly once after WebKit finishes saving.
    unsafe {
        webkit2gtk_sys::webkit_user_content_filter_store_save(
            store,
            c"aegis-content-rules".as_ptr(),
            bytes,
            ptr::null_mut(),
            Some(prepare_finished),
            Box::into_raw(context).cast(),
        );
    }
    Ok(PreparedContentRules { filter })
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
    webview: &WebView,
    on_shortcut: Box<dyn Fn(Shortcut)>,
) -> Result<(), FfiError> {
    webview.webview().connect_key_press_event(move |_, event| {
        let state = event.state() & !(gdk::ModifierType::LOCK_MASK | gdk::ModifierType::MOD2_MASK);
        let allowed = gdk::ModifierType::CONTROL_MASK
            | gdk::ModifierType::SHIFT_MASK
            | gdk::ModifierType::MOD1_MASK;
        if !(state & !allowed).is_empty() {
            return glib::Propagation::Proceed;
        }
        let mods = Mods {
            ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
            shift: state.contains(gdk::ModifierType::SHIFT_MASK),
            alt: state.contains(gdk::ModifierType::MOD1_MASK),
            command: false,
        };
        let key = match event.keyval().to_lower() {
            gdk::keys::constants::l => Key::L,
            gdk::keys::constants::t => Key::T,
            gdk::keys::constants::w => Key::W,
            gdk::keys::constants::r => Key::R,
            gdk::keys::constants::s => Key::S,
            gdk::keys::constants::comma => Key::Comma,
            gdk::keys::constants::Tab | gdk::keys::constants::ISO_Left_Tab => Key::Tab,
            gdk::keys::constants::F5 => Key::F5,
            gdk::keys::constants::Left => Key::Left,
            gdk::keys::constants::Right => Key::Right,
            _ => return glib::Propagation::Proceed,
        };
        let Some(shortcut) = match_shortcut(key, mods, Platform::Linux) else {
            return glib::Propagation::Proceed;
        };
        on_shortcut(shortcut);
        glib::Propagation::Stop
    });
    Ok(())
}

pub(super) fn go_back(webview: &WebView) -> Result<bool, FfiError> {
    if !webview.webview().can_go_back() {
        return Ok(false);
    }
    webview.webview().go_back();
    Ok(true)
}

pub(super) fn go_forward(webview: &WebView) -> Result<(), FfiError> {
    webview.webview().go_forward();
    Ok(())
}

pub(super) fn focus(webview: &WebView) -> Result<(), FfiError> {
    webview.webview().grab_focus();
    Ok(())
}

pub(super) fn window_glass_supported(
    _window: &impl wry::raw_window_handle::HasWindowHandle,
) -> bool {
    false
}

pub(super) fn apply_window_material(
    _window: &impl wry::raw_window_handle::HasWindowHandle,
    _material: super::WindowMaterial,
    _dark: bool,
) -> Result<(), FfiError> {
    Ok(())
}

pub(super) fn set_page_corner_radius(_webview: &WebView, _radius: f64) -> Result<(), FfiError> {
    Ok(())
}

pub(super) fn reduce_motion() -> bool {
    use gtk::prelude::GtkSettingsExt;

    gtk::Settings::default().is_some_and(|settings| !settings.is_gtk_enable_animations())
}

pub(super) fn set_smooth_scrolling(webview: &WebView, enabled: bool) {
    use webkit2gtk::SettingsExt;

    // gtk widgets also have settings, so name the webkit one
    if let Some(settings) = WebViewExt::settings(&webview.webview()) {
        settings.set_enable_smooth_scrolling(enabled);
    }
}

pub(super) fn set_hardware_acceleration(webview: &WebView, enabled: bool) {
    use webkit2gtk::SettingsExt;

    if let Some(settings) = WebViewExt::settings(&webview.webview()) {
        if !enabled {
            settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
        }
    }
}
