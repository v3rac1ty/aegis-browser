//! the only crate allowed to use unsafe for native platform APIs

mod content_rules;
#[cfg_attr(target_os = "windows", allow(dead_code))]
mod shortcut_keys;

use std::fmt;
use wry::WebView;

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
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    Reload,
    Back,
    Forward,
    ToggleCompact,
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

/// focus the native webview
pub fn focus(webview: &WebView) -> Result<(), FfiError> {
    platform::focus(webview)
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
