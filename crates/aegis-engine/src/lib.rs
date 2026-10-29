//! safe content view API and platform webview backends
#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use wry::raw_window_handle::HasWindowHandle;
#[cfg(target_os = "linux")]
use wry::raw_window_handle::RawWindowHandle;
use wry::{WebView, WebViewBuilder};

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

/// errors from the content view
#[derive(Debug)]
pub enum EngineError {
    WebView(wry::Error),
    WindowHandle(wry::raw_window_handle::HandleError),
    UnsupportedWindowSystem,
    InvalidUrl,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebView(error) => write!(f, "webview error: {error}"),
            Self::WindowHandle(error) => write!(f, "window handle error: {error}"),
            Self::UnsupportedWindowSystem => {
                write!(
                    f,
                    "Linux child webviews require an Xlib window on X11 or XWayland"
                )
            }
            Self::InvalidUrl => write!(f, "enter an http:// or https:// URL with a host"),
        }
    }
}

impl Error for EngineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WebView(error) => Some(error),
            Self::WindowHandle(error) => Some(error),
            Self::UnsupportedWindowSystem | Self::InvalidUrl => None,
        }
    }
}

impl From<wry::Error> for EngineError {
    fn from(error: wry::Error) -> Self {
        Self::WebView(error)
    }
}

/// a native child webview
pub struct ContentView {
    webview: WebView,
}

impl fmt::Debug for ContentView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentView").finish_non_exhaustive()
    }
}

impl ContentView {
    /// create a child view inside a native window
    pub fn new(
        parent: &impl HasWindowHandle,
        bounds: Bounds,
        start: Start,
    ) -> Result<Self, EngineError> {
        #[cfg(target_os = "linux")]
        {
            let handle = parent.window_handle().map_err(EngineError::WindowHandle)?;
            if !matches!(handle.as_raw(), RawWindowHandle::Xlib(_)) {
                return Err(EngineError::UnsupportedWindowSystem);
            }
        }

        let builder = WebViewBuilder::new()
            .with_bounds(bounds.as_wry_rect())
            .with_devtools(cfg!(debug_assertions))
            .with_autoplay(false);

        let builder = match start {
            Start::Url(url) => {
                if !is_http_url(&url) {
                    return Err(EngineError::InvalidUrl);
                }
                builder.with_url(url)
            }
            Start::Html(html) => builder.with_html(html),
        };

        let webview = builder.build_as_child(parent)?;
        Ok(Self { webview })
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

    /// load an http or https URL
    pub fn navigate(&self, url: &str) -> Result<(), EngineError> {
        if !is_http_url(url) {
            return Err(EngineError::InvalidUrl);
        }
        self.webview.load_url(url)?;
        Ok(())
    }

    /// access the native wry webview
    pub fn webview(&self) -> &WebView {
        &self.webview
    }
}

/// check whether a string is an http or https URL with a host
pub fn is_http_url(value: &str) -> bool {
    let Ok(uri) = value.parse::<wry::http::Uri>() else {
        return false;
    };
    matches!(uri.scheme_str(), Some("http" | "https")) && uri.host().is_some()
}
