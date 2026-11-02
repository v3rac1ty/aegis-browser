//! the only crate allowed to use unsafe for native platform APIs

mod content_rules;
mod os_key;
#[cfg_attr(target_os = "windows", allow(dead_code))]
mod shortcut_keys;

pub use os_key::{OsKeyProtector, OwnerCheck, owner_check_available, verify_owner};
use std::fmt;
use wry::WebView;
use wry::raw_window_handle::HasWindowHandle;

/// material behind the transparent app surface
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowMaterial {
    Glass,
    Solid,
}

/// substring rules used by the native blocking spike
#[derive(Debug, Default)]
pub struct BlockRules {
    pub url_substrings: Vec<String>,
}

/// result of a native suspend request
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendOutcome {
    Suspended,
    Failed,
    Unsupported,
}

/// shortcuts intercepted inside the native webview
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    CommandBar,
    Settings,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    Reload,
    Back,
    Forward,
    ToggleCompact,
    LockVault,
}

/// raw native events before app validation
#[derive(Debug)]
pub enum NativeEvent {
    FaviconChanged {
        url: String,
        bytes: Vec<u8>,
    },
    FaviconRgba {
        url: String,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
    },
    NavigationFailed {
        url: String,
        reason: NativeFailReason,
    },
    ContentCrashed,
    PermissionRequested {
        id: u64,
        origin: String,
        kind: NativePermissionKind,
    },
    DownloadRequested {
        id: u64,
        url: String,
        suggested_name: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeFailReason {
    Canceled,
    CertificateError,
    ConnectionFailed,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePermissionKind {
    Camera,
    Microphone,
    Location,
    Notifications,
    ClipboardRead,
    Other,
}

/// pending native decisions for one view
pub struct NativeViewState(platform::NativeViewState);

/// compiled content rules shared by every view in an engine
pub struct PreparedContentRules(platform::PreparedContentRules);

impl Clone for PreparedContentRules {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceContext {
    Document,
    Script,
    Stylesheet,
    Image,
    Font,
    Media,
    Xhr,
    Websocket,
    Ping,
    Other,
}

pub type RequestFilter = Box<dyn Fn(&str, ResourceContext) -> bool>;

impl fmt::Debug for PreparedContentRules {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedContentRules")
            .field("ready", &self.is_ready())
            .finish()
    }
}

pub fn prepare_content_rules(
    json_lists: Vec<String>,
    on_ready: Box<dyn FnOnce(Result<(), FfiError>)>,
) -> Result<PreparedContentRules, FfiError> {
    platform::prepare_content_rules(json_lists, on_ready).map(PreparedContentRules)
}

impl PreparedContentRules {
    pub fn is_ready(&self) -> bool {
        self.0.is_ready()
    }

    pub fn attach(
        &self,
        webview: &WebView,
        should_block: RequestFilter,
        on_blocked: Box<dyn Fn(&str)>,
    ) -> Result<(), FfiError> {
        self.0.attach(webview, should_block, on_blocked)
    }

    pub fn set_blocking(&self, webview: &WebView, enabled: bool) -> Result<(), FfiError> {
        self.0.set_blocking(webview, enabled)
    }
}

impl fmt::Debug for NativeViewState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeViewState").finish_non_exhaustive()
    }
}

pub fn install_native_hooks(
    webview: &WebView,
    on_event: Box<dyn Fn(NativeEvent)>,
) -> Result<NativeViewState, FfiError> {
    platform::install_native_hooks(webview, on_event).map(NativeViewState)
}

impl NativeViewState {
    pub fn answer_permission(&self, id: u64, allow: bool, remember: bool) -> Result<(), FfiError> {
        self.0.answer_permission(id, allow, remember)
    }

    pub fn answer_download(&self, id: u64, accept: bool) -> Result<(), FfiError> {
        self.0.answer_download(id, accept)
    }
}

/// error from a native hook
#[derive(Debug)]
pub enum FfiError {
    Native(String),
}

impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(message) => write!(f, "native hook error: {message}"),
        }
    }
}

impl std::error::Error for FfiError {}

/// install request blocking rules on a webview
pub fn install_block_rules(
    webview: &WebView,
    rules: &BlockRules,
    on_blocked: Box<dyn Fn(&str)>,
) -> Result<(), FfiError> {
    platform::install_block_rules(webview, rules, on_blocked)
}

/// ask the native engine to suspend a webview
pub fn suspend(webview: &WebView, done: Box<dyn FnOnce(SuspendOutcome)>) -> Result<(), FfiError> {
    platform::suspend(webview, done)
}

/// wake a suspended webview
pub fn resume(webview: &WebView) -> Result<(), FfiError> {
    platform::resume(webview)
}

/// hint that the webview should use less memory in the background
pub fn set_background_memory(webview: &WebView, low: bool) -> Result<(), FfiError> {
    platform::set_background_memory(webview, low)
}

/// intercept native webview shortcuts
pub fn install_shortcuts(
    webview: &WebView,
    on_shortcut: Box<dyn Fn(Shortcut)>,
) -> Result<(), FfiError> {
    platform::install_shortcuts(webview, on_shortcut)
}

/// returns whether backward navigation started
pub fn go_back(webview: &WebView) -> Result<bool, FfiError> {
    platform::go_back(webview)
}

/// navigate forward when history permits
pub fn go_forward(webview: &WebView) -> Result<(), FfiError> {
    platform::go_forward(webview)
}

/// set smooth scrolling in a page, only webkitgtk exposes the setting
pub fn set_smooth_scrolling(webview: &WebView, enabled: bool) {
    // webview2 reads a launch flag, wkwebview has no setting so pages keep the system scrolling
    #[cfg(target_os = "linux")]
    platform::set_smooth_scrolling(webview, enabled);
    #[cfg(not(target_os = "linux"))]
    let _ = (webview, enabled);
}

/// set page hardware acceleration where the platform exposes it
pub fn set_hardware_acceleration(webview: &WebView, enabled: bool) {
    #[cfg(target_os = "linux")]
    platform::set_hardware_acceleration(webview, enabled);
    #[cfg(not(target_os = "linux"))]
    let _ = (webview, enabled);
}

/// focus the native webview
pub fn focus(webview: &WebView) -> Result<(), FfiError> {
    platform::focus(webview)
}

/// set the native window backdrop
pub fn apply_window_material(
    window: &impl HasWindowHandle,
    material: WindowMaterial,
    dark: bool,
) -> Result<(), FfiError> {
    platform::apply_window_material(window, material, dark)
}

/// whether the system can draw window glass
pub fn window_glass_supported(window: &impl HasWindowHandle) -> bool {
    platform::window_glass_supported(window)
}

/// clip the page view to rounded corners
pub fn set_page_corner_radius(webview: &WebView, radius: f64) -> Result<(), FfiError> {
    platform::set_page_corner_radius(webview, radius)
}

/// whether the system requests reduced animation
pub fn reduce_motion() -> bool {
    platform::reduce_motion()
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod win;

#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use win as platform;
