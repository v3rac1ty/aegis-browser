//! safe content view API and platform webview backends
#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;

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

/// a raw event from one content view
#[derive(Debug)]
pub struct EngineEvent {
    pub key: u64,
    pub kind: EventKind,
}

/// changes reported by the native engine
#[derive(Debug)]
pub enum EventKind {
    TitleChanged(String),
    UrlChanged(String),
    LoadStarted,
    LoadFinished,
    NewWindowRequested(String),
    NavigationBlocked(String),
}

/// errors from the content view
#[derive(Debug)]
pub enum EngineError {
    WebView(wry::Error),
    Native(aegis_ffi::FfiError),
    WindowHandle(wry::raw_window_handle::HandleError),
    UnsupportedWindowSystem,
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
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WebView(error) => Some(error),
            Self::Native(error) => Some(error),
            Self::WindowHandle(error) => Some(error),
            Self::UnsupportedWindowSystem => None,
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
    pub fn new(data_dir: PathBuf, wake: Box<dyn Fn()>) -> Self {
        Self {
            context: WebContext::new(Some(data_dir.join("webview"))),
            events: Rc::new(RefCell::new(VecDeque::new())),
            wake: Rc::from(wake),
        }
    }

    /// create a child view in the shared context
    pub fn create_view(
        &mut self,
        parent: &impl HasWindowHandle,
        key: u64,
        bounds: Bounds,
        start: Start,
        visible: bool,
        policy: ViewPolicy,
    ) -> Result<ContentView, EngineError> {
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
                if (policy.allow_navigation)(&url) {
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

        let builder = match start {
            Start::Url(url) => builder.with_url(url),
            Start::Html(html) => builder.with_html(html),
        };
        let webview = builder.build_as_child(parent)?;
        Ok(ContentView { key, webview })
    }

    /// take all events reported since the previous drain
    pub fn drain_events(&self) -> Vec<EngineEvent> {
        self.events.borrow_mut().drain(..).collect()
    }
}

fn push_event(events: &EventQueue, wake: &Rc<dyn Fn()>, key: u64, kind: EventKind) {
    events.borrow_mut().push_back(EngineEvent { key, kind });
    wake();
}

/// a native child webview
pub struct ContentView {
    key: u64,
    webview: WebView,
}

impl fmt::Debug for ContentView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentView")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl ContentView {
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
}
