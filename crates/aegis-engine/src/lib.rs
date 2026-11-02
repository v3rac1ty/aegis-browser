//! safe content view API and platform webview backends
#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;

pub use aegis_ffi::PreparedContentRules;
pub use aegis_ffi::RequestFilter;
pub use aegis_ffi::ResourceContext;
#[cfg(target_os = "macos")]
use wry::WebViewBuilderExtDarwin;
#[cfg(target_os = "windows")]
use wry::WebViewBuilderExtWindows;
use wry::raw_window_handle::HasWindowHandle;
#[cfg(target_os = "linux")]
use wry::raw_window_handle::RawWindowHandle;
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebView, WebViewBuilder};

/// the content view rectangle in logical pixels
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    fn as_wry_rect(self) -> wry::Rect {
        wry::Rect {
            position: wry::dpi::LogicalPosition::new(self.x, self.y).into(),
            size: wry::dpi::LogicalSize::new(self.width, self.height).into(),
        }
    }
}

/// the first content loaded by a view
#[derive(Debug)]
pub enum Start {
    Url(String),
    Html(String),
}

/// navigation policy supplied by the app
pub struct ViewPolicy {
    pub allow_navigation: Rc<dyn Fn(&str) -> bool>,
}

impl fmt::Debug for ViewPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewPolicy").finish_non_exhaustive()
    }
}

/// inputs for creating one child content view
pub struct ViewRequest<'a> {
    pub key: u64,
    pub bounds: Bounds,
    pub start: Start,
    pub visible: bool,
    pub policy: ViewPolicy,
    pub rules: &'a PreparedContentRules,
    pub block_trackers: bool,
    pub should_block: RequestFilter,
    pub on_blocked: Box<dyn Fn(&str)>,
}

impl fmt::Debug for ViewRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewRequest")
            .field("key", &self.key)
            .field("bounds", &self.bounds)
            .field("start", &self.start)
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

/// a raw event from one content view
#[derive(Debug)]
pub struct EngineEvent {
    pub key: u64,
    pub kind: EventKind,
}

/// changes reported by the native engine
#[derive(Debug)]
pub enum EventKind {
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
    TitleChanged(String),
    UrlChanged(String),
    LoadStarted,
    LoadFinished,
    NewWindowRequested(String),
    NavigationBlocked(String),
    NavigationFailed {
        url: String,
        reason: FailReason,
    },
    ContentCrashed,
    PermissionRequested {
        id: u64,
        origin: String,
        kind: PermissionKind,
    },
    DownloadRequested {
        id: u64,
        url: String,
        suggested_name: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailReason {
    Canceled,
    CertificateError,
    ConnectionFailed,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionKind {
    Camera,
    Microphone,
    Location,
    Notifications,
    ClipboardRead,
    Other,
}

/// errors from the content view
#[derive(Debug)]
pub enum EngineError {
    WebView(wry::Error),
    Native(aegis_ffi::FfiError),
    WindowHandle(wry::raw_window_handle::HandleError),
    UnsupportedWindowSystem,
    RulesNotReady,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebView(error) => write!(f, "webview error: {error}"),
            Self::Native(error) => write!(f, "native webview error: {error}"),
            Self::WindowHandle(error) => write!(f, "window handle error: {error}"),
            Self::UnsupportedWindowSystem => write!(
                f,
                "Linux child webviews require an Xlib window on X11 or XWayland"
            ),
            Self::RulesNotReady => write!(f, "content rules are not ready"),
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WebView(error) => Some(error),
            Self::Native(error) => Some(error),
            Self::WindowHandle(error) => Some(error),
            Self::UnsupportedWindowSystem | Self::RulesNotReady => None,
        }
    }
}

impl From<wry::Error> for EngineError {
    fn from(error: wry::Error) -> Self {
        Self::WebView(error)
    }
}

impl From<aegis_ffi::FfiError> for EngineError {
    fn from(error: aegis_ffi::FfiError) -> Self {
        Self::Native(error)
    }
}

type EventQueue = Rc<RefCell<VecDeque<EngineEvent>>>;

/// shared web context and pending view events
pub struct Engine {
    context: WebContext,
    events: EventQueue,
    wake: Rc<dyn Fn()>,
    smooth_scrolling: bool,
    hardware_acceleration: bool,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// create a shared context with webview data in the app data directory
    pub fn new(
        data_dir: PathBuf,
        wake: Box<dyn Fn()>,
        smooth_scrolling: bool,
        hardware_acceleration: bool,
    ) -> Self {
        Self {
            context: WebContext::new(Some(data_dir.join("webview"))),
            events: Rc::new(RefCell::new(VecDeque::new())),
            wake: Rc::from(wake),
            smooth_scrolling,
            hardware_acceleration,
        }
    }

    /// set smooth scrolling for views created from now on
    pub fn set_smooth_scrolling(&mut self, enabled: bool) {
        // webview2 fixes its browser flags when the shared environment starts
        if cfg!(target_os = "windows") {
            return;
        }
        self.smooth_scrolling = enabled;
    }

    /// compile rule lists once and call on_ready when they can be attached
    pub fn prepare_content_rules(
        &self,
        json_lists: Vec<String>,
        on_ready: Box<dyn FnOnce(Result<(), EngineError>)>,
    ) -> Result<PreparedContentRules, EngineError> {
        Ok(aegis_ffi::prepare_content_rules(
            json_lists,
            Box::new(move |result| {
                on_ready(result.map_err(EngineError::Native));
            }),
        )?)
    }

    /// create a child view only after rules are ready; rules attach before its first load
    pub fn create_view(
        &mut self,
        parent: &impl HasWindowHandle,
        request: ViewRequest<'_>,
    ) -> Result<ContentView, EngineError> {
        let ViewRequest {
            key,
            bounds,
            start,
            visible,
            policy,
            rules,
            block_trackers,
            should_block,
            on_blocked,
        } = request;
        if !rules.is_ready() {
            return Err(EngineError::RulesNotReady);
        }
        let blocking = Rc::new(Cell::new(block_trackers));
        #[cfg(target_os = "linux")]
        {
            let handle = parent.window_handle().map_err(EngineError::WindowHandle)?;
            if !matches!(handle.as_raw(), RawWindowHandle::Xlib(_)) {
                return Err(EngineError::UnsupportedWindowSystem);
            }
        }

        let title_events = self.events.clone();
        let title_wake = self.wake.clone();
        let load_events = self.events.clone();
        let load_wake = self.wake.clone();
        let navigation_events = self.events.clone();
        let navigation_wake = self.wake.clone();
        let popup_events = self.events.clone();
        let popup_wake = self.wake.clone();
        let native_events = self.events.clone();
        let native_wake = self.wake.clone();

        let builder = WebViewBuilder::new_with_web_context(&mut self.context)
            .with_bounds(bounds.as_wry_rect())
            .with_visible(visible)
            .with_devtools(cfg!(debug_assertions))
            .with_autoplay(false)
            .with_document_title_changed_handler(move |title| {
                push_event(
                    &title_events,
                    &title_wake,
                    key,
                    EventKind::TitleChanged(title),
                );
            })
            .with_on_page_load_handler(move |event, url| {
                let kind = match event {
                    PageLoadEvent::Started => EventKind::LoadStarted,
                    PageLoadEvent::Finished => EventKind::LoadFinished,
                };
                push_event(&load_events, &load_wake, key, kind);
                push_event(&load_events, &load_wake, key, EventKind::UrlChanged(url));
            })
            .with_navigation_handler(move |url| {
                // the preliminary blank load must not consume a test page allowance
                if url == "about:blank" || (policy.allow_navigation)(&url) {
                    return true;
                }
                push_event(
                    &navigation_events,
                    &navigation_wake,
                    key,
                    EventKind::NavigationBlocked(url),
                );
                false
            })
            .with_new_window_req_handler(move |url, _features| {
                push_event(
                    &popup_events,
                    &popup_wake,
                    key,
                    EventKind::NewWindowRequested(url),
                );
                NewWindowResponse::Deny
            });

        #[cfg(target_os = "macos")]
        let builder = builder.with_ipc_handler({
            let events = self.events.clone();
            let wake = self.wake.clone();
            move |request| {
                let message = request.body();
                if let Some(payload) = message.strip_prefix("aegis-favicon:")
                    && let Some((url, encoded)) = payload.split_once('\n')
                    && let Some(bytes) = decode_favicon_base64(encoded)
                {
                    push_event(
                        &events,
                        &wake,
                        key,
                        EventKind::FaviconChanged {
                            url: url.to_owned(),
                            bytes,
                        },
                    );
                }
            }
        });

        #[cfg(target_os = "windows")]
        let builder = builder
            .with_additional_browser_args(browser_args(
                self.smooth_scrolling,
                self.hardware_acceleration,
            ))
            .with_default_context_menus(true)
            .with_browser_accelerator_keys(true);
        #[cfg(target_os = "macos")]
        let builder = builder
            .with_permission_handler(|_| wry::PermissionResponse::Deny)
            .with_on_web_content_process_terminate_handler({
                let events = native_events.clone();
                let wake = native_wake.clone();
                move || push_event(&events, &wake, key, EventKind::ContentCrashed)
            });
        #[cfg(not(target_os = "windows"))]
        let builder = builder.with_download_started_handler({
            let events = native_events.clone();
            let wake = native_wake.clone();
            let next_id = std::cell::Cell::new(1u64);
            move |url, path| {
                let id = next_id.get();
                next_id.set(id.saturating_add(1));
                let suggested_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("download")
                    .to_owned();
                push_event(
                    &events,
                    &wake,
                    key,
                    EventKind::DownloadRequested {
                        id,
                        url,
                        suggested_name,
                    },
                );
                false
            }
        });
        // install native hooks before the first app navigation
        let builder = builder.with_url("about:blank");
        let webview = builder.build_as_child(parent)?;
        aegis_ffi::set_smooth_scrolling(&webview, self.smooth_scrolling);
        aegis_ffi::set_hardware_acceleration(&webview, self.hardware_acceleration);
        let native = aegis_ffi::install_native_hooks(
            &webview,
            Box::new(move |event| {
                let kind = match event {
                    aegis_ffi::NativeEvent::FaviconChanged { url, bytes } => {
                        EventKind::FaviconChanged { url, bytes }
                    }
                    aegis_ffi::NativeEvent::FaviconRgba {
                        url,
                        rgba,
                        width,
                        height,
                    } => EventKind::FaviconRgba {
                        url,
                        rgba,
                        width,
                        height,
                    },
                    aegis_ffi::NativeEvent::NavigationFailed { url, reason } => {
                        EventKind::NavigationFailed {
                            url,
                            reason: match reason {
                                aegis_ffi::NativeFailReason::Canceled => FailReason::Canceled,
                                aegis_ffi::NativeFailReason::CertificateError => {
                                    FailReason::CertificateError
                                }
                                aegis_ffi::NativeFailReason::ConnectionFailed => {
                                    FailReason::ConnectionFailed
                                }
                                aegis_ffi::NativeFailReason::Other => FailReason::Other,
                            },
                        }
                    }
                    aegis_ffi::NativeEvent::ContentCrashed => EventKind::ContentCrashed,
                    aegis_ffi::NativeEvent::PermissionRequested { id, origin, kind } => {
                        EventKind::PermissionRequested {
                            id,
                            origin,
                            kind: match kind {
                                aegis_ffi::NativePermissionKind::Camera => PermissionKind::Camera,
                                aegis_ffi::NativePermissionKind::Microphone => {
                                    PermissionKind::Microphone
                                }
                                aegis_ffi::NativePermissionKind::Location => {
                                    PermissionKind::Location
                                }
                                aegis_ffi::NativePermissionKind::Notifications => {
                                    PermissionKind::Notifications
                                }
                                aegis_ffi::NativePermissionKind::ClipboardRead => {
                                    PermissionKind::ClipboardRead
                                }
                                aegis_ffi::NativePermissionKind::Other => PermissionKind::Other,
                            },
                        }
                    }
                    aegis_ffi::NativeEvent::DownloadRequested {
                        id,
                        url,
                        suggested_name,
                    } => EventKind::DownloadRequested {
                        id,
                        url,
                        suggested_name,
                    },
                };
                push_event(&native_events, &native_wake, key, kind);
            }),
        )?;
        if cfg!(target_os = "windows") || block_trackers {
            let filter_blocking = blocking.clone();
            rules.attach(
                &webview,
                Box::new(move |url, context| filter_blocking.get() && should_block(url, context)),
                on_blocked,
            )?;
        }
        match start {
            Start::Url(url) => webview.load_url(&url)?,
            Start::Html(html) => webview.load_html(&html)?,
        }
        Ok(ContentView {
            key,
            native,
            webview,
            rules: rules.clone(),
            blocking,
        })
    }

    /// take all events reported since the previous drain
    pub fn drain_events(&self) -> Vec<EngineEvent> {
        self.events.borrow_mut().drain(..).collect()
    }
}

// webview2 flags, smooth scrolling is the only one that varies
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn browser_args(smooth_scrolling: bool, hardware_acceleration: bool) -> String {
    let mut args = String::from(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --test-third-party-cookie-phaseout",
    );
    if !smooth_scrolling {
        args.push_str(" --disable-smooth-scrolling");
    }
    if !hardware_acceleration {
        args.push_str(" --disable-gpu");
    }
    args
}

fn push_event(events: &EventQueue, wake: &Rc<dyn Fn()>, key: u64, kind: EventKind) {
    events.borrow_mut().push_back(EngineEvent { key, kind });
    wake();
}

/// a native child webview
pub struct ContentView {
    key: u64,
    native: aegis_ffi::NativeViewState,
    webview: WebView,
    rules: PreparedContentRules,
    blocking: Rc<Cell<bool>>,
}

impl fmt::Debug for ContentView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentView")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl ContentView {
    #[cfg(target_os = "macos")]
    pub fn fetch_favicon(&self) -> Result<(), EngineError> {
        // wkwebview has no favicon API, so the page fetches its own same-origin PNG
        self.eval_with_result(MACOS_FAVICON_SCRIPT, |_| {})
    }
    /// update tracker blocking on this view
    pub fn set_blocking(&self, enabled: bool) -> Result<(), EngineError> {
        if self.blocking.get() == enabled {
            return Ok(());
        }
        self.rules.set_blocking(&self.webview, enabled)?;
        self.blocking.set(enabled);
        Ok(())
    }
    /// update smooth scrolling in this page, only webkitgtk has the setting
    pub fn set_smooth_scrolling(&self, enabled: bool) {
        aegis_ffi::set_smooth_scrolling(&self.webview, enabled);
    }
    /// evaluates a script and returns its json result
    pub fn eval_with_result(
        &self,
        js: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<(), EngineError> {
        self.webview.evaluate_script_with_callback(js, callback)?;
        Ok(())
    }
    /// return the app's opaque view key
    pub fn key(&self) -> u64 {
        self.key
    }

    /// move or resize the child view
    pub fn set_bounds(&self, bounds: Bounds) -> Result<(), EngineError> {
        self.webview.set_bounds(bounds.as_wry_rect())?;
        Ok(())
    }

    /// show or hide the child view
    pub fn set_visible(&self, visible: bool) -> Result<(), EngineError> {
        self.webview.set_visible(visible)?;
        Ok(())
    }

    /// load a URL supplied by the app
    pub fn navigate(&self, url: &str) -> Result<(), EngineError> {
        self.webview.load_url(url)?;
        Ok(())
    }

    /// reload the current page
    pub fn reload(&self) -> Result<(), EngineError> {
        self.webview.reload()?;
        Ok(())
    }

    /// returns whether backward navigation started
    pub fn go_back(&self) -> Result<bool, EngineError> {
        Ok(aegis_ffi::go_back(&self.webview)?)
    }

    /// navigate forward when history permits
    pub fn go_forward(&self) -> Result<(), EngineError> {
        aegis_ffi::go_forward(&self.webview)?;
        Ok(())
    }

    /// focus the native webview
    pub fn focus(&self) -> Result<(), EngineError> {
        aegis_ffi::focus(&self.webview)?;
        Ok(())
    }

    /// access the native wry webview
    pub fn webview(&self) -> &WebView {
        &self.webview
    }

    pub fn answer_permission(
        &self,
        id: u64,
        allow: bool,
        remember: bool,
    ) -> Result<(), EngineError> {
        self.native.answer_permission(id, allow, remember)?;
        Ok(())
    }

    pub fn answer_download(&self, id: u64, accept: bool) -> Result<(), EngineError> {
        self.native.answer_download(id, accept)?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
const MACOS_FAVICON_SCRIPT: &str = r#"(() => {
  const pageUrl = location.href;
  const links = document.querySelectorAll('link[rel]');
  for (const link of links) {
    if (!link.rel.toLowerCase().split(/\s+/).includes('icon')) continue;
    let url;
    try { url = new URL(link.href, pageUrl); } catch { continue; }
    if (url.origin !== new URL(pageUrl).origin || !/^https?:$/.test(url.protocol)) continue;
    if (link.type.toLowerCase() !== 'image/png' && !url.pathname.toLowerCase().endsWith('.png')) continue;
    fetch(url.href, { credentials: 'same-origin' }).then(async response => {
      if (!response.ok || !response.body) return;
      const reader = response.body.getReader();
      const chunks = [];
      let length = 0;
      while (true) {
        const part = await reader.read();
        if (part.done) break;
        length += part.value.length;
        if (length > 1048576) { await reader.cancel(); return; }
        chunks.push(part.value);
      }
      let binary = '';
      for (const chunk of chunks) {
        for (let i = 0; i < chunk.length; i += 8192) {
          binary += String.fromCharCode(...chunk.subarray(i, i + 8192));
        }
      }
      window.ipc.postMessage('aegis-favicon:' + pageUrl + '\n' + btoa(binary));
    }).catch(() => {});
    break;
  }
})()"#;

#[cfg(target_os = "macos")]
fn decode_favicon_base64(encoded: &str) -> Option<Vec<u8>> {
    if encoded.len() > 1_398_104 || !encoded.len().is_multiple_of(4) {
        return None;
    }
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(encoded.len() / 4 * 3);
    for chunk in encoded.as_bytes().as_chunks::<4>().0 {
        let a = value(chunk[0])?;
        let b = value(chunk[1])?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            value(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            value(chunk[3])?
        };
        if chunk[2] == b'=' && chunk[3] != b'=' {
            return None;
        }
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c << 6) | d);
        }
    }
    (out.len() <= 1024 * 1024).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::browser_args;

    #[test]
    fn browser_args_keep_existing_flags() {
        for (smooth_scrolling, hardware_acceleration) in
            [(true, true), (true, false), (false, true), (false, false)]
        {
            let args = browser_args(smooth_scrolling, hardware_acceleration);
            assert!(
                args.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection")
            );
            assert!(args.contains("--test-third-party-cookie-phaseout"));
            assert_eq!(
                args.contains("--disable-smooth-scrolling"),
                !smooth_scrolling
            );
            assert_eq!(args.contains("--disable-gpu"), !hardware_acceleration);
        }
    }
}
