use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use aegis_core::{Browser, Command, Effect, LoadState, Placement, TabId};
use aegis_engine::{
    Bounds, ContentView, Engine, EventKind, FailReason, PermissionKind, PreparedContentRules,
    ResourceContext, Start, ViewPolicy, ViewRequest,
};
use aegis_ffi::Shortcut;
use aegis_ipc::PopupLimiter;
use aegis_shield::{ResourceKind, Shield, Upgrade};
use eframe::egui;

use crate::security::{self, SecurityStore};

struct PendingPermission {
    id: u64,
    origin: String,
    kind: PermissionKind,
    remember: bool,
}

struct PendingDownload {
    id: u64,
    host: String,
    name: String,
}

#[derive(Default)]
struct NavigationState {
    real_navigation_started: HashSet<TabId>,
    waiting: HashMap<TabId, (String, String)>,
    continue_once: HashMap<TabId, String>,
    interstitial: HashMap<TabId, (String, String)>,
    crashed: Vec<TabId>,
}

impl NavigationState {
    fn take_once(&mut self, tab: TabId, url: &str) -> bool {
        if self
            .continue_once
            .get(&tab)
            .is_some_and(|value| value == url)
        {
            self.continue_once.remove(&tab);
            true
        } else {
            false
        }
    }
}

pub enum ExecuteResult {
    Done,
    Command(Command),
    Deferred(TabId),
}

pub enum OverlayAction {
    GoBack(TabId),
    Navigate(TabId, String),
    Reload(TabId),
}

fn view_should_be_visible(active: bool, settings_open: bool) -> bool {
    active && !settings_open
}

fn prompt_card(
    ctx: &egui::Context,
    content: egui::Rect,
    theme: crate::theme::Theme,
    id: &'static str,
    mut add: impl FnMut(&mut egui::Ui),
) {
    let window = ctx
        .input(|input| input.viewport().inner_rect)
        .unwrap_or(content);
    let builder = egui::ViewportBuilder::default()
        .with_icon(crate::logo::icon())
        .with_position(egui::pos2(
            window.min.x + content.center().x - 190.0,
            window.min.y + content.top() + 16.0,
        ))
        .with_inner_size([380.0, 150.0])
        .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
        .with_has_shadow(false)
        .with_decorations(false)
        .with_resizable(false)
        .with_taskbar(false)
        .with_always_on_top()
        .with_active(false);
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(id), builder, |ctx, _| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme.palette.overlay)
                    .corner_radius(theme.metrics.overlay_radius)
                    .inner_margin(18.0)
                    .show(ui, |ui| {
                        ui.set_min_width(340.0);
                        add(ui);
                    });
            });
    });
}

pub struct Views {
    content: HashMap<TabId, ContentView>,
    loading: HashMap<TabId, bool>,
    blocked: Rc<RefCell<HashMap<TabId, u32>>>,
    shortcuts: Rc<RefCell<VecDeque<Shortcut>>>,
    popup_limiter: PopupLimiter,
    bounds: Option<Bounds>,
    window_scale: Option<f64>,
    test_page: Option<TabId>,
    rules: Option<PreparedContentRules>,
    rule_error: Rc<RefCell<Option<String>>>,
    shield: Option<Rc<Shield>>,
    store: Rc<RefCell<SecurityStore>>,
    https_only: Rc<Cell<bool>>,
    block_trackers: Cell<bool>,
    blocking_update_failed: bool,
    navigation: Rc<RefCell<NavigationState>>,
    upgrades: Rc<RefCell<VecDeque<(TabId, String)>>>,
    page_urls: HashMap<TabId, Rc<RefCell<String>>>,
    permissions: HashMap<TabId, VecDeque<PendingPermission>>,
    downloads: HashMap<TabId, VecDeque<PendingDownload>>,
}

struct BlockingTransition {
    update_views: Vec<TabId>,
    enabled: bool,
}

impl BlockingTransition {
    fn reported_active(&self, update_failed: bool) -> bool {
        self.enabled && !update_failed
    }
}

fn blocking_transition(previous: bool, enabled: bool, live: &[TabId]) -> BlockingTransition {
    BlockingTransition {
        update_views: if previous == enabled {
            Vec::new()
        } else {
            live.to_vec()
        },
        enabled,
    }
}

impl Views {
    pub fn new(data_dir: std::path::PathBuf, https_only: bool, block_trackers: bool) -> Self {
        Self {
            content: HashMap::new(),
            loading: HashMap::new(),
            blocked: Rc::new(RefCell::new(HashMap::new())),
            shortcuts: Rc::new(RefCell::new(VecDeque::new())),
            popup_limiter: PopupLimiter::new(),
            bounds: None,
            window_scale: None,
            test_page: None,
            rules: None,
            rule_error: Rc::new(RefCell::new(None)),
            shield: None,
            store: Rc::new(RefCell::new(SecurityStore::load(data_dir))),
            https_only: Rc::new(Cell::new(https_only)),
            block_trackers: Cell::new(block_trackers),
            blocking_update_failed: false,
            navigation: Rc::new(RefCell::new(NavigationState::default())),
            upgrades: Rc::new(RefCell::new(VecDeque::new())),
            page_urls: HashMap::new(),
            permissions: HashMap::new(),
            downloads: HashMap::new(),
        }
    }

    /// update smooth scrolling in open pages
    pub fn set_smooth_scrolling(&self, enabled: bool) {
        for view in self.content.values() {
            view.set_smooth_scrolling(enabled);
        }
    }

    pub fn set_https_only(&self, enabled: bool) {
        let previous = self.https_only.replace(enabled);
        if previous && !enabled {
            let mut navigation = self.navigation.borrow_mut();
            navigation.waiting.clear();
            navigation.interstitial.clear();
            navigation.continue_once.clear();
        }
    }

    pub fn set_block_trackers(&mut self, enabled: bool) -> Option<String> {
        if self.block_trackers.get() == enabled {
            return None;
        }
        let live: Vec<_> = self.content.keys().copied().collect();
        let transition = blocking_transition(self.block_trackers.get(), enabled, &live);
        self.block_trackers.set(enabled);
        let mut error = None;
        for tab in &transition.update_views {
            if let Some(view) = self.content.get(tab) {
                if let Err(failure) = view.set_blocking(enabled) {
                    error.get_or_insert_with(|| failure.to_string());
                    continue;
                }
                if let Err(failure) = view.reload() {
                    error.get_or_insert_with(|| failure.to_string());
                }
            }
        }
        self.blocking_update_failed = !transition.reported_active(error.is_some()) && enabled;
        error
            .map(|error| format!("Tracker blocking could not be applied to all open tabs: {error}"))
    }

    pub fn blocking_reported(&self) -> bool {
        self.block_trackers.get() && !self.blocking_update_failed
    }

    pub fn clear_exceptions(&self) -> Result<(), String> {
        self.store.borrow_mut().clear_hosts()
    }

    pub fn prepare(
        &mut self,
        engine: &Engine,
        shield: Option<Shield>,
        json: Vec<String>,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        *self.rule_error.borrow_mut() = None;
        self.shield = shield.map(Rc::new);
        let errors = self.rule_error.clone();
        let repaint = ctx.clone();
        self.rules = Some(
            engine
                .prepare_content_rules(
                    json,
                    Box::new(move |result| {
                        if let Err(error) = result {
                            *errors.borrow_mut() = Some(error.to_string());
                        }
                        repaint.request_repaint();
                    }),
                )
                .map_err(|error| error.to_string())?,
        );
        Ok(())
    }

    pub fn rules_ready(&self) -> bool {
        self.rules
            .as_ref()
            .is_some_and(PreparedContentRules::is_ready)
    }

    pub fn take_rule_error(&self) -> Option<String> {
        self.rule_error.borrow_mut().take()
    }

    pub fn show_overlays(
        &mut self,
        ctx: &egui::Context,
        content: egui::Rect,
        theme: crate::theme::Theme,
        active: Option<TabId>,
    ) -> (Option<OverlayAction>, Option<String>) {
        let Some(tab) = active else {
            return (None, None);
        };
        if self.navigation.borrow().crashed.contains(&tab) {
            let mut reload = false;
            egui::Area::new(egui::Id::unique("crash_card"))
                .order(egui::Order::Foreground)
                .fixed_pos(content.center() - egui::vec2(190.0, 70.0))
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(theme.palette.overlay)
                        .corner_radius(theme.metrics.overlay_radius)
                        .inner_margin(20.0)
                        .show(ui, |ui| {
                            ui.set_min_width(340.0);
                            ui.heading("This page stopped working");
                            reload = ui.button("Reload").clicked();
                        });
                });
            return (reload.then_some(OverlayAction::Reload(tab)), None);
        }
        if let Some((original, _)) = self.navigation.borrow().interstitial.get(&tab) {
            let host = security::host(original).unwrap_or("site").to_owned();
            let display_host = aegis_ipc::sanitize_title(&host);
            let mut choice = 0;
            egui::Area::new(egui::Id::unique("https_interstitial"))
                .order(egui::Order::Foreground)
                .fixed_pos(content.center() - egui::vec2(220.0, 120.0))
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(theme.palette.overlay)
                        .corner_radius(theme.metrics.overlay_radius)
                        .inner_margin(24.0)
                        .show(ui, |ui| {
                            ui.set_min_width(390.0);
                            ui.heading(crate::protections::HTTPS_FAILURE_TITLE);
                            ui.label(
                                egui::RichText::new(&display_host).color(theme.palette.secondary),
                            );
                            ui.add_space(16.0);
                            if ui
                                .add(egui::Button::new("Go back").fill(theme.accent))
                                .clicked()
                            {
                                choice = 1;
                            }
                            if ui.button("Continue once over HTTP").clicked() {
                                choice = 2;
                            }
                            if ui
                                .button(format!("Always allow for {display_host}"))
                                .clicked()
                            {
                                choice = 3;
                            }
                        });
                });
            if choice == 0 {
                return (None, None);
            }
            let original = self
                .navigation
                .borrow_mut()
                .interstitial
                .remove(&tab)
                .map(|entry| entry.0);
            if let Some(original) = original {
                if choice == 1 {
                    return (Some(OverlayAction::GoBack(tab)), None);
                }
                if choice == 2 {
                    self.navigation
                        .borrow_mut()
                        .continue_once
                        .insert(tab, original.clone());
                } else if let Err(error) = self.store.borrow_mut().allow_host(&host) {
                    return (None, Some(error));
                }
                return (Some(OverlayAction::Navigate(tab, original)), None);
            }
        }
        if let Some(prompt) = self
            .permissions
            .get_mut(&tab)
            .and_then(|queue| queue.front_mut())
        {
            let mut answer = None;
            prompt_card(ctx, content, theme, "permission_card", |ui| {
                let origin = if prompt.origin.is_empty() {
                    "This site".to_owned()
                } else {
                    aegis_ipc::sanitize_title(&prompt.origin)
                };
                ui.label(format!(
                    "{origin} wants to use your {}",
                    security::permission_name(prompt.kind)
                ));
                if !prompt.origin.is_empty() {
                    ui.checkbox(&mut prompt.remember, "Remember for this site");
                }
                ui.horizontal(|ui| {
                    if ui.button("Allow").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Don't allow").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if let Some(allow) = answer
                && let Some(prompt) = self.permissions.get_mut(&tab).and_then(VecDeque::pop_front)
            {
                if let Some(view) = self.content.get(&tab) {
                    let _ = view.answer_permission(prompt.id, allow, false);
                }
                if prompt.remember
                    && !prompt.origin.is_empty()
                    && let Err(error) = self.store.borrow_mut().remember_permission(
                        &prompt.origin,
                        prompt.kind,
                        allow,
                    )
                {
                    return (None, Some(error));
                }
            }
        } else if let Some(prompt) = self.downloads.get(&tab).and_then(|queue| queue.front()) {
            let mut answer = None;
            prompt_card(ctx, content, theme, "download_card", |ui| {
                ui.label(format!("Download {} from {}?", prompt.name, prompt.host));
                ui.horizontal(|ui| {
                    if ui.button("Download").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if let Some(accept) = answer
                && let Some(prompt) = self.downloads.get_mut(&tab).and_then(VecDeque::pop_front)
            {
                if let Some(view) = self.content.get(&tab)
                    && let Err(error) = view.answer_download(prompt.id, accept)
                {
                    return (None, Some(error.to_string()));
                }
                if accept {
                    return (None, Some("Saved to Downloads".to_owned()));
                }
            }
        }
        (None, None)
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

    pub fn active_hidden(&self, browser: &Browser) -> bool {
        let Some(tab) = browser.active_tab() else {
            return false;
        };
        self.active_interstitial_or_crash(tab)
    }

    fn active_interstitial_or_crash(&self, tab: TabId) -> bool {
        let state = self.navigation.borrow();
        state.interstitial.contains_key(&tab) || state.crashed.contains(&tab)
    }

    pub fn active_loading(&self, browser: &Browser) -> bool {
        browser
            .active_tab()
            .and_then(|id| self.loading.get(&id))
            .copied()
            .unwrap_or(false)
    }

    pub fn active_blocked(&self, browser: &Browser) -> u32 {
        browser
            .active_tab()
            .and_then(|id| self.blocked.borrow().get(&id).copied())
            .unwrap_or(0)
    }

    pub fn take_shortcuts(&self) -> Vec<Shortcut> {
        self.shortcuts.borrow_mut().drain(..).collect()
    }

    fn deny_prompts(&mut self, tab: TabId) {
        if let Some(view) = self.content.get(&tab) {
            if let Some(mut pending) = self.permissions.remove(&tab) {
                for prompt in pending.drain(..) {
                    let _ = view.answer_permission(prompt.id, false, false);
                }
            }
            if let Some(mut pending) = self.downloads.remove(&tab) {
                for prompt in pending.drain(..) {
                    let _ = view.answer_download(prompt.id, false);
                }
            }
        }
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
        settings_open: bool,
    ) -> Result<ExecuteResult, String> {
        match effect {
            Effect::CreateView { tab, url } => {
                if let Some(error) = self.rule_error.borrow().as_ref() {
                    return Err(error.clone());
                }
                let Some(rules) = self.rules.as_ref().filter(|rules| rules.is_ready()) else {
                    return Ok(ExecuteResult::Deferred(tab));
                };
                let Some(window) = frame.winit_window() else {
                    return Ok(ExecuteResult::Deferred(tab));
                };
                let Some(bounds) = self.bounds else {
                    return Ok(ExecuteResult::Deferred(tab));
                };
                let test_page = self.test_page == Some(tab);
                let page_url = Rc::new(RefCell::new(url.clone()));
                let start = if test_page {
                    Start::Html(include_str!("test_page.html").to_owned())
                } else {
                    Start::Url(url)
                };
                let initial_load_done = Cell::new(false);
                let navigation = self.navigation.clone();
                let upgrades = self.upgrades.clone();
                let store = self.store.clone();
                let https_only = self.https_only.clone();
                let policy = ViewPolicy {
                    allow_navigation: Rc::new(move |target| {
                        if test_page && !initial_load_done.replace(true) {
                            return true;
                        }
                        if !aegis_ipc::is_allowed_navigation(target) {
                            return false;
                        }
                        if navigation.borrow_mut().take_once(tab, target) {
                            return true;
                        }
                        let result = security::decision(
                            target,
                            https_only.get(),
                            &store.borrow().exceptions,
                        );
                        if let Upgrade::To(secure) = result {
                            navigation
                                .borrow_mut()
                                .waiting
                                .insert(tab, (secure.clone(), target.to_owned()));
                            upgrades.borrow_mut().push_back((tab, secure));
                            return false;
                        }
                        let keep_waiting = navigation
                            .borrow()
                            .waiting
                            .get(&tab)
                            .is_some_and(|(secure, _)| secure == target);
                        if !keep_waiting {
                            navigation.borrow_mut().waiting.remove(&tab);
                        }
                        true
                    }),
                };
                let shield = self.shield.clone();
                let block_trackers = self.block_trackers.get();
                let filter_page_url = page_url.clone();
                let view = engine
                    .create_view(
                        window.as_ref(),
                        ViewRequest {
                            key: tab.get(),
                            bounds,
                            start,
                            visible: view_should_be_visible(active == Some(tab), settings_open),
                            policy,
                            rules,
                            block_trackers,
                            should_block: Box::new(move |request_url, context| {
                                if !block_trackers {
                                    return false;
                                }
                                if test_page
                                    && cfg!(debug_assertions)
                                    && request_url.contains("aegis-block")
                                {
                                    return true;
                                }
                                let Some(shield) = shield.as_ref() else {
                                    return false;
                                };
                                let kind = match context {
                                    ResourceContext::Document => ResourceKind::Document,
                                    ResourceContext::Script => ResourceKind::Script,
                                    ResourceContext::Stylesheet => ResourceKind::Stylesheet,
                                    ResourceContext::Image => ResourceKind::Image,
                                    ResourceContext::Font => ResourceKind::Font,
                                    ResourceContext::Media => ResourceKind::Media,
                                    ResourceContext::Xhr => ResourceKind::Xhr,
                                    ResourceContext::Websocket => ResourceKind::Websocket,
                                    ResourceContext::Ping => ResourceKind::Ping,
                                    ResourceContext::Other => ResourceKind::Other,
                                };
                                shield.should_block(request_url, &filter_page_url.borrow(), kind)
                            }),
                            on_blocked: Box::new({
                                let blocked = self.blocked.clone();
                                let repaint = ctx.clone();
                                move |_| {
                                    let mut counts = blocked.borrow_mut();
                                    let count = counts.entry(tab).or_default();
                                    *count = count.saturating_add(1);
                                    repaint.request_repaint();
                                }
                            }),
                        },
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
                if view_should_be_visible(active == Some(tab), settings_open) {
                    view.focus().map_err(|error| error.to_string())?;
                }
                self.content.insert(tab, view);
                self.page_urls.insert(tab, page_url);
                self.loading.insert(tab, true);
                Ok(ExecuteResult::Command(Command::SetLoadState {
                    tab,
                    state: LoadState::Live,
                }))
            }
            Effect::DestroyView(tab) => {
                self.deny_prompts(tab);
                self.content.remove(&tab);
                self.loading.remove(&tab);
                self.blocked.borrow_mut().remove(&tab);
                self.popup_limiter.forget(tab.get());
                self.page_urls.remove(&tab);
                let mut navigation = self.navigation.borrow_mut();
                navigation.real_navigation_started.remove(&tab);
                navigation.waiting.remove(&tab);
                navigation.interstitial.remove(&tab);
                navigation.continue_once.remove(&tab);
                navigation.crashed.retain(|id| *id != tab);
                Ok(ExecuteResult::Done)
            }
            Effect::ShowView(tab) => {
                if let Some(view) = self.content.get(&tab) {
                    let visible = view_should_be_visible(active == Some(tab), settings_open)
                        && !self.active_interstitial_or_crash(tab);
                    view.set_visible(visible)
                        .map_err(|error| error.to_string())?;
                    if visible {
                        view.focus().map_err(|error| error.to_string())?;
                    }
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
                    let visible = view_should_be_visible(active == Some(tab), settings_open)
                        && !self.active_interstitial_or_crash(tab);
                    view.set_visible(visible)
                        .map_err(|error| error.to_string())?;
                    if visible {
                        view.focus().map_err(|error| error.to_string())?;
                    }
                }
                Ok(ExecuteResult::Command(Command::SetLoadState {
                    tab,
                    state: LoadState::Live,
                }))
            }
            Effect::NavigateView { tab, url } => {
                if let Some(view) = self.content.get(&tab) {
                    self.navigation
                        .borrow_mut()
                        .real_navigation_started
                        .insert(tab);
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
        ctx: &egui::Context,
    ) -> (Vec<Command>, Vec<String>) {
        let mut commands = Vec::new();
        let mut notices = Vec::new();
        for event in engine.drain_events() {
            let tab = TabId::from_raw(event.key);
            if browser.tab(tab).is_none() || !self.content.contains_key(&tab) {
                continue;
            }
            match event.kind {
                EventKind::FaviconChanged { url: source, bytes } => {
                    let url = self.page_urls.get(&tab).map(|url| url.borrow());
                    let host = url
                        .as_deref()
                        .and_then(|url| favicon_host_if_current(&source, url));
                    if let (Some(host), Ok(icon)) = (host, aegis_ipc::decode_favicon(&bytes)) {
                        crate::favicons::save(ctx, &host, icon);
                    }
                }
                EventKind::FaviconRgba {
                    url: source,
                    rgba,
                    width,
                    height,
                } => {
                    let url = self.page_urls.get(&tab).map(|url| url.borrow());
                    let host = url
                        .as_deref()
                        .and_then(|url| favicon_host_if_current(&source, url));
                    if let (Some(host), Ok(icon)) = (
                        host,
                        aegis_ipc::downscale_favicon(&rgba, width as usize, height as usize),
                    ) {
                        crate::favicons::save(ctx, &host, icon);
                    }
                }
                EventKind::TitleChanged(raw) => commands.push(Command::SetTitle {
                    tab,
                    title: aegis_ipc::sanitize_title(&raw),
                }),
                EventKind::UrlChanged(raw) => {
                    let started = self
                        .navigation
                        .borrow()
                        .real_navigation_started
                        .contains(&tab);
                    if should_ignore_initial_blank(&raw, started) {
                        continue;
                    }
                    if let Ok(url) = aegis_ipc::page_url(&raw) {
                        if raw != "about:blank" {
                            self.navigation
                                .borrow_mut()
                                .real_navigation_started
                                .insert(tab);
                        }
                        if let Some(page_url) = self.page_urls.get(&tab) {
                            *page_url.borrow_mut() = raw.clone();
                        }
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
                            opener: Some(tab),
                        });
                    }
                }
                EventKind::NavigationBlocked(raw) => {
                    if self
                        .navigation
                        .borrow()
                        .waiting
                        .get(&tab)
                        .is_some_and(|(_, original)| original == &raw)
                    {
                        continue;
                    }
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
                EventKind::NavigationFailed { url, reason } => {
                    if url != "about:blank" {
                        self.navigation
                            .borrow_mut()
                            .real_navigation_started
                            .insert(tab);
                    }
                    let started = self
                        .navigation
                        .borrow()
                        .real_navigation_started
                        .contains(&tab);
                    if !counts_as_failure(&url, reason, started) {
                        #[cfg(debug_assertions)]
                        eprintln!("nav failed: url={url} reason={reason:?} branch=ignored");
                        continue;
                    }
                    self.loading.insert(tab, false);
                    let matched = self
                        .navigation
                        .borrow()
                        .waiting
                        .get(&tab)
                        .is_some_and(|(secure, _)| secure == &url);
                    #[cfg(debug_assertions)]
                    {
                        let branch = if matched { "interstitial" } else { "notice" };
                        eprintln!("nav failed: url={url} reason={reason:?} branch={branch}");
                    }
                    if matched {
                        if let Some((secure, original)) =
                            self.navigation.borrow_mut().waiting.remove(&tab)
                        {
                            if let Some(view) = self.content.get(&tab) {
                                let _ = view.set_visible(false);
                            }
                            self.navigation
                                .borrow_mut()
                                .interstitial
                                .insert(tab, (original, secure));
                        }
                    } else {
                        notices.push("Page failed to load".to_owned());
                    }
                }
                EventKind::ContentCrashed => {
                    self.loading.insert(tab, false);
                    self.deny_prompts(tab);
                    if let Some(view) = self.content.get(&tab) {
                        let _ = view.set_visible(false);
                    }
                    self.navigation.borrow_mut().crashed.push(tab);
                }
                EventKind::PermissionRequested { id, origin, kind } => {
                    if let Some(view) = self.content.get(&tab) {
                        let origin = security::permission_origin(&origin).unwrap_or_default();
                        if let Some(allow) = self.store.borrow().permission(&origin, kind) {
                            let _ = view.answer_permission(id, allow, false);
                        } else {
                            self.permissions
                                .entry(tab)
                                .or_default()
                                .push_back(PendingPermission {
                                    id,
                                    origin,
                                    kind,
                                    remember: false,
                                });
                        }
                    }
                }
                EventKind::DownloadRequested {
                    id,
                    url,
                    suggested_name,
                } => {
                    if cfg!(target_os = "windows") {
                        self.downloads
                            .entry(tab)
                            .or_default()
                            .push_back(PendingDownload {
                                id,
                                host: aegis_ipc::sanitize_title(
                                    security::host(&url).unwrap_or("site"),
                                ),
                                name: aegis_ipc::sanitize_title(&suggested_name),
                            });
                    } else {
                        notices.push("Downloads are not available on this platform yet".to_owned());
                    }
                }
                EventKind::LoadStarted => {
                    self.deny_prompts(tab);
                    self.loading.insert(tab, true);
                    self.blocked.borrow_mut().insert(tab, 0);
                }
                EventKind::LoadFinished => {
                    self.loading.insert(tab, false);
                    #[cfg(target_os = "macos")]
                    if let Some(view) = self.content.get(&tab) {
                        let _ = view.fetch_favicon();
                    }
                }
            }
        }
        for (tab, secure) in self.upgrades.borrow_mut().drain(..) {
            if browser.tab(tab).is_some() {
                commands.push(Command::Navigate { tab, url: secure });
            }
        }
        (commands, notices)
    }
}

// a canceled navigation was replaced or upgraded, the page did not fail
// the initial blank load is aborted when the real url starts
fn counts_as_failure(url: &str, reason: FailReason, real_navigation_started: bool) -> bool {
    (url != "about:blank" || real_navigation_started) && reason != FailReason::Canceled
}

fn should_ignore_initial_blank(url: &str, real_navigation_started: bool) -> bool {
    url == "about:blank" && !real_navigation_started
}

fn favicon_host_if_current(source: &str, current: &str) -> Option<String> {
    let source = aegis_ipc::page_url(source).ok()?;
    let current = aegis_ipc::page_url(current).ok()?;
    if source.host() != current.host() {
        return None;
    }
    Some(aegis_ipc::display_host(&source))
}

pub(crate) fn navigation_changes_host(before: &str, after: &str) -> bool {
    security::host(before) != security::host(after)
}

#[cfg(test)]
mod tests {
    use super::{
        NavigationState, blocking_transition, counts_as_failure, favicon_host_if_current,
        navigation_changes_host, should_ignore_initial_blank, view_should_be_visible,
    };
    use aegis_core::TabId;
    use aegis_engine::FailReason;

    #[test]
    fn canceled_navigations_are_not_failures() {
        let url = "https://example.com/";
        assert!(!counts_as_failure(url, FailReason::Canceled, false));
        assert!(counts_as_failure(url, FailReason::ConnectionFailed, false));
        assert!(counts_as_failure(url, FailReason::CertificateError, false));
        assert!(counts_as_failure(url, FailReason::Other, false));
    }

    #[test]
    fn initial_blank_events_are_ignored_but_later_blank_navigation_is_real() {
        assert!(should_ignore_initial_blank("about:blank", false));
        assert!(!should_ignore_initial_blank("about:blank", true));
        assert!(!counts_as_failure("about:blank", FailReason::Other, false));
        assert!(counts_as_failure("about:blank", FailReason::Other, true));
        assert!(!counts_as_failure(
            "about:blank",
            FailReason::Canceled,
            true
        ));
        assert!(counts_as_failure(
            "https://example.com/",
            FailReason::ConnectionFailed,
            false
        ));
    }

    #[test]
    fn restored_tab_keeps_its_url_through_initial_blank_event() {
        let mut browser = aegis_core::Browser::new();
        let _ = browser.apply(
            aegis_core::Command::OpenTab {
                url: "https://saved.test/".to_owned(),
                placement: aegis_core::Placement::End,
                activate: true,
                opener: None,
            },
            0,
        );
        let Some(tab) = browser.active_tab() else {
            panic!("restored tab did not open")
        };
        if !should_ignore_initial_blank("about:blank", false) {
            let _ = browser.apply(
                aegis_core::Command::SetUrl {
                    tab,
                    url: "about:blank".to_owned(),
                },
                1,
            );
        }
        let Some(restored) = browser.tab(tab) else {
            panic!("restored tab disappeared")
        };
        assert_eq!(restored.url(), "https://saved.test/");
    }

    #[test]
    fn favicon_is_saved_only_for_the_current_host() {
        assert!(
            favicon_host_if_current("https://example.test/icon.png", "https://example.test/page")
                .is_some()
        );
        assert!(
            favicon_host_if_current("https://old.test/icon.png", "https://new.test/page").is_none()
        );
        assert!(favicon_host_if_current("bad url", "https://example.test/page").is_none());
        assert!(favicon_host_if_current("https://example.test/icon.png", "bad url").is_none());
    }

    #[test]
    fn view_visibility_requires_active_tab_and_closed_settings() {
        assert!(view_should_be_visible(true, false));
        assert!(!view_should_be_visible(true, true));
        assert!(!view_should_be_visible(false, false));
        assert!(!view_should_be_visible(false, true));
    }

    #[test]
    fn continue_once_is_exact_and_consumed() {
        let tab = TabId::from_raw(1);
        let mut state = NavigationState::default();
        state
            .continue_once
            .insert(tab, "http://example.com/".to_owned());
        assert!(!state.take_once(tab, "http://example.com/other"));
        assert!(state.take_once(tab, "http://example.com/"));
        assert!(!state.take_once(tab, "http://example.com/"));
    }

    #[test]
    fn blocking_transition_updates_all_live_views_and_reports_failures() {
        let live = [TabId::from_raw(1), TabId::from_raw(2)];
        let enabled = blocking_transition(false, true, &live);
        assert_eq!(enabled.update_views, live);
        assert!(enabled.reported_active(false));
        assert!(!enabled.reported_active(true));

        let unchanged = blocking_transition(true, true, &live);
        assert!(unchanged.update_views.is_empty());

        let disabled = blocking_transition(true, false, &live);
        assert_eq!(disabled.update_views, live);
        assert!(!disabled.reported_active(false));
    }

    #[test]
    fn navigation_to_new_host_clears_stale_title() {
        assert!(navigation_changes_host(
            "https://www.google.com/search?q=ivr",
            "https://illinivexu.web.illinois.edu/"
        ));
        assert!(!navigation_changes_host(
            "https://illinivexu.web.illinois.edu/",
            "https://illinivexu.web.illinois.edu/about"
        ));
        let mut browser = aegis_core::Browser::new();
        let _ = browser.apply(
            aegis_core::Command::OpenTab {
                url: "https://www.google.com/search?q=ivr".to_owned(),
                placement: aegis_core::Placement::End,
                activate: true,
                opener: None,
            },
            0,
        );
        let Some(tab) = browser.active_tab() else {
            panic!("tab did not open")
        };
        let _ = browser.apply(
            aegis_core::Command::SetTitle {
                tab,
                title: "ivr - Google Search".to_owned(),
            },
            0,
        );
        let before = browser
            .tab(tab)
            .map(|tab| tab.url())
            .unwrap_or_default()
            .to_owned();
        let after = "https://illinivexu.web.illinois.edu/";
        let _ = browser.apply(
            aegis_core::Command::SetUrl {
                tab,
                url: after.to_owned(),
            },
            0,
        );
        if navigation_changes_host(&before, after) {
            let _ = browser.apply(
                aegis_core::Command::SetTitle {
                    tab,
                    title: String::new(),
                },
                0,
            );
        }
        let Some(current) = browser.tab(tab) else {
            panic!("tab disappeared")
        };
        assert!(current.title().is_empty());
        assert_eq!(
            crate::sidebar::display_host(current.url()),
            "illinivexu.web.illinois.edu"
        );
        assert_eq!(
            crate::sidebar::avatar(&crate::sidebar::display_host(current.url())).0,
            'I'
        );
    }
}
