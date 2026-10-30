use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use aegis_core::{Browser, Command, Effect, LoadState, Placement, TabId};
use aegis_engine::{Bounds, ContentView, Engine, EventKind, Start, ViewPolicy};
use aegis_ffi::{BlockRules, Shortcut};
use aegis_ipc::PopupLimiter;
use eframe::egui;

pub enum ExecuteResult {
    Done,
    Command(Command),
    Deferred(TabId),
}

pub struct Views {
    content: HashMap<TabId, ContentView>,
    loading: HashMap<TabId, bool>,
    shortcuts: Rc<RefCell<VecDeque<Shortcut>>>,
    popup_limiter: PopupLimiter,
    bounds: Option<Bounds>,
    window_scale: Option<f64>,
    test_page: Option<TabId>,
}

impl Views {
    pub fn new() -> Self {
        Self {
            content: HashMap::new(),
            loading: HashMap::new(),
            shortcuts: Rc::new(RefCell::new(VecDeque::new())),
            popup_limiter: PopupLimiter::new(),
            bounds: None,
            window_scale: None,
            test_page: None,
        }
    }

    pub fn set_test_page(&mut self, tab: TabId) {
        self.test_page = Some(tab);
    }

    pub fn active(&self, browser: &Browser) -> Option<&ContentView> {
        browser.active_tab().and_then(|id| self.content.get(&id))
    }

    pub fn has_view(&self, tab: TabId) -> bool {
        self.content.contains_key(&tab)
    }

    pub fn active_loading(&self, browser: &Browser) -> bool {
        browser
            .active_tab()
            .and_then(|id| self.loading.get(&id))
            .copied()
            .unwrap_or(false)
    }

    pub fn take_shortcuts(&self) -> Vec<Shortcut> {
        self.shortcuts.borrow_mut().drain(..).collect()
    }

    pub fn set_bounds(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        rect: egui::Rect,
    ) -> Result<(), String> {
        let Some(window) = frame.winit_window() else {
            return Ok(());
        };
        let logical_per_point = f64::from(ctx.pixels_per_point()) / window.scale_factor();
        let bounds = Bounds {
            x: f64::from(rect.min.x) * logical_per_point,
            y: f64::from(rect.min.y) * logical_per_point,
            width: f64::from(rect.width()) * logical_per_point,
            height: f64::from(rect.height()) * logical_per_point,
        };
        if self.bounds == Some(bounds) && self.window_scale == Some(window.scale_factor()) {
            return Ok(());
        }
        for view in self.content.values() {
            view.set_bounds(bounds).map_err(|error| error.to_string())?;
        }
        self.bounds = Some(bounds);
        self.window_scale = Some(window.scale_factor());
        Ok(())
    }

    pub fn execute(
        &mut self,
        effect: Effect,
        engine: &mut Engine,
        frame: &eframe::Frame,
        ctx: &egui::Context,
        active: Option<TabId>,
    ) -> Result<ExecuteResult, String> {
        match effect {
            Effect::CreateView { tab, url } => {
                let Some(window) = frame.winit_window() else {
                    return Ok(ExecuteResult::Deferred(tab));
                };
                let Some(bounds) = self.bounds else {
                    return Ok(ExecuteResult::Deferred(tab));
                };
                let start = if self.test_page == Some(tab) {
                    Start::Html(include_str!("test_page.html").to_owned())
                } else {
                    Start::Url(url)
                };
                let policy = if self.test_page == Some(tab) {
                    let initial_load_done = Rc::new(Cell::new(false));
                    ViewPolicy {
                        allow_navigation: Rc::new(move |url| {
                            // allow the test page's first data navigation only
                            if !initial_load_done.replace(true) {
                                return true;
                            }
                            aegis_ipc::is_allowed_navigation(url)
                        }),
                    }
                } else {
                    ViewPolicy {
                        allow_navigation: Rc::new(aegis_ipc::is_allowed_navigation),
                    }
                };
                let view = engine
                    .create_view(
                        window.as_ref(),
                        tab.get(),
                        bounds,
                        start,
                        active == Some(tab),
                        policy,
                    )
                    .map_err(|error| error.to_string())?;
                aegis_ffi::install_block_rules(
                    view.webview(),
                    &BlockRules {
                        url_substrings: vec!["aegis-block".to_owned()],
                    },
                    Box::new(|_| {}),
                )
                .map_err(|error| error.to_string())?;
                let shortcuts = self.shortcuts.clone();
                let repaint = ctx.clone();
                aegis_ffi::install_shortcuts(
                    view.webview(),
                    Box::new(move |shortcut| {
                        shortcuts.borrow_mut().push_back(shortcut);
                        repaint.request_repaint();
                    }),
                )
                .map_err(|error| error.to_string())?;
                if active == Some(tab) {
                    view.focus().map_err(|error| error.to_string())?;
                }
                self.content.insert(tab, view);
                self.loading.insert(tab, true);
                Ok(ExecuteResult::Command(Command::SetLoadState {
                    tab,
                    state: LoadState::Live,
                }))
            }
            Effect::DestroyView(tab) => {
                self.content.remove(&tab);
                self.loading.remove(&tab);
                self.popup_limiter.forget(tab.get());
                Ok(ExecuteResult::Done)
            }
            Effect::ShowView(tab) => {
                if let Some(view) = self.content.get(&tab) {
                    view.set_visible(true).map_err(|error| error.to_string())?;
                    view.focus().map_err(|error| error.to_string())?;
                }
                Ok(ExecuteResult::Done)
            }
            Effect::HideView(tab) => {
                if let Some(view) = self.content.get(&tab) {
                    view.set_visible(false).map_err(|error| error.to_string())?;
                }
                Ok(ExecuteResult::Done)
            }
            Effect::ResumeView(tab) => {
                if let Some(view) = self.content.get(&tab) {
                    aegis_ffi::resume(view.webview()).map_err(|error| error.to_string())?;
                    view.set_visible(true).map_err(|error| error.to_string())?;
                    view.focus().map_err(|error| error.to_string())?;
                }
                Ok(ExecuteResult::Command(Command::SetLoadState {
                    tab,
                    state: LoadState::Live,
                }))
            }
            Effect::NavigateView { tab, url } => {
                if let Some(view) = self.content.get(&tab) {
                    view.navigate(&url).map_err(|error| error.to_string())?;
                }
                Ok(ExecuteResult::Done)
            }
            Effect::PersistSession | Effect::GroupCreated(_) | Effect::WorkspaceCreated(_) => {
                Ok(ExecuteResult::Done)
            }
        }
    }

    pub fn drain_events(
        &mut self,
        engine: &Engine,
        browser: &Browser,
        now_ms: u64,
    ) -> (Vec<Command>, Vec<String>) {
        let mut commands = Vec::new();
        let mut notices = Vec::new();
        for event in engine.drain_events() {
            let tab = TabId::from_raw(event.key);
            if browser.tab(tab).is_none() || !self.content.contains_key(&tab) {
                continue;
            }
            match event.kind {
                EventKind::TitleChanged(raw) => commands.push(Command::SetTitle {
                    tab,
                    title: aegis_ipc::sanitize_title(&raw),
                }),
                EventKind::UrlChanged(raw) => {
                    if let Ok(url) = aegis_ipc::page_url(&raw) {
                        commands.push(Command::SetUrl {
                            tab,
                            url: url.into_string(),
                        });
                    }
                }
                EventKind::NewWindowRequested(raw) => {
                    if let Ok(url) = aegis_ipc::page_url(&raw)
                        && self.popup_limiter.allow(event.key, now_ms)
                    {
                        commands.push(Command::OpenTab {
                            url: url.into_string(),
                            placement: Placement::AfterActive,
                            activate: true,
                        });
                    }
                }
                EventKind::NavigationBlocked(raw) => {
                    let scheme = raw.split(':').next().unwrap_or("");
                    if !scheme.is_empty()
                        && scheme.len() <= 32
                        && scheme
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
                    {
                        notices.push(format!("Blocked navigation to {scheme}:"));
                    } else {
                        notices.push("Blocked navigation".to_owned());
                    }
                }
                EventKind::LoadStarted => {
                    self.loading.insert(tab, true);
                }
                EventKind::LoadFinished => {
                    self.loading.insert(tab, false);
                }
            }
        }
        (commands, notices)
    }
}
