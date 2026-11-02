use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use aegis_core::{
    Browser, Command, Effect, Glass, GroupColor, Layout, LoadState, Placement, Side, TabId,
    ThemeMode, session,
};
use aegis_engine::{Engine, Start};
use aegis_ffi::{Shortcut, WindowMaterial};
use aegis_shield::{Shield, content_blocker_json};
use eframe::egui;

#[derive(Clone, Copy, Debug)]
pub struct PaneLayout {
    pub sidebar_rect: egui::Rect,
    pub page_rect: egui::Rect,
    pub handle_rect: egui::Rect,
}

pub fn pane_layout(window: egui::Rect, sidebar_width: f32, side: Side, gap: f32) -> PaneLayout {
    let top = window.top() + gap;
    let bottom = window.bottom() - gap;
    match side {
        Side::Left => {
            let sidebar_rect = egui::Rect::from_min_max(
                egui::pos2(window.left() + gap, top),
                egui::pos2(window.left() + gap + sidebar_width, bottom),
            );
            let page_rect = egui::Rect::from_min_max(
                egui::pos2(sidebar_rect.right() + gap, top),
                egui::pos2(window.right() - gap, bottom),
            );
            let handle_rect =
                egui::Rect::from_min_max(sidebar_rect.right_top(), page_rect.left_bottom());
            PaneLayout {
                sidebar_rect,
                page_rect,
                handle_rect,
            }
        }
        Side::Right => {
            let sidebar_rect = egui::Rect::from_min_max(
                egui::pos2(window.right() - gap - sidebar_width, top),
                egui::pos2(window.right() - gap, bottom),
            );
            let page_rect = egui::Rect::from_min_max(
                egui::pos2(window.left() + gap, top),
                egui::pos2(sidebar_rect.left() - gap, bottom),
            );
            let handle_rect =
                egui::Rect::from_min_max(page_rect.right_top(), sidebar_rect.left_bottom());
            PaneLayout {
                sidebar_rect,
                page_rect,
                handle_rect,
            }
        }
    }
}

pub(crate) fn pane_layout_at_scale(
    window: egui::Rect,
    width: f32,
    side: Side,
    scale: f32,
) -> PaneLayout {
    let window = egui::Rect::from_min_max(
        (window.min.to_vec2() * scale).round().to_pos2(),
        (window.max.to_vec2() * scale).round().to_pos2(),
    );
    let layout = pane_layout(window, (width * scale).round(), side, (8.0 * scale).round());
    PaneLayout {
        sidebar_rect: egui::Rect::from_min_max(
            layout.sidebar_rect.min / scale,
            layout.sidebar_rect.max / scale,
        ),
        page_rect: egui::Rect::from_min_max(
            layout.page_rect.min / scale,
            layout.page_rect.max / scale,
        ),
        handle_rect: egui::Rect::from_min_max(
            layout.handle_rect.min / scale,
            layout.handle_rect.max / scale,
        ),
    }
}

fn stale_title_tab(browser: &Browser, command: &Command) -> Option<TabId> {
    match command {
        Command::Navigate { tab, url } | Command::SetUrl { tab, url } => browser
            .tab(*tab)
            .filter(|current| {
                !current.title().is_empty()
                    && crate::views::navigation_changes_host(current.url(), url)
            })
            .map(|_| *tab),
        _ => None,
    }
}

fn classic_page_layout(content: egui::Rect, scale: f32) -> egui::Rect {
    let left = (content.left() * scale).round();
    let right = (content.right() * scale).round();
    let top = (content.top() * scale).round();
    let bottom = (content.bottom() * scale).round();
    let gap = (8.0 * scale).round();
    egui::Rect::from_min_max(
        egui::pos2((left + gap) / scale, top / scale),
        egui::pos2((right - gap) / scale, (bottom - gap) / scale),
    )
}

use crate::command_bar::{self, BarAction, CommandBar, Mode, SuggestionAction};
use crate::compact::Compact;
use crate::favicons;
use crate::fonts;
use crate::glass;
use crate::imports::{self, Imports};
use crate::passwords::{self, Passwords, ProtectedAction};
use crate::persist::SessionSaver;
use crate::settings_page;
use crate::sidebar::{Action, Sidebar};
use crate::tabstrip::{CompactTop, TabStrip};
use crate::theme;
use crate::views::{ExecuteResult, OverlayAction, Views};
use crate::welcome::Welcome;

type ProtectionLoad = Receiver<Result<(Shield, Vec<String>), String>>;

pub struct AegisApp {
    browser: Browser,
    // drop views before the engine so the shared web context stays live
    views: Views,
    engine: Option<Engine>,
    data_dir: PathBuf,
    directory_error: Option<String>,
    protection_loader: Option<ProtectionLoad>,
    tracker_loaded: bool,
    tracker_missing: bool,
    rule_fallback_attempted: bool,
    sidebar: Sidebar,
    tabstrip: TabStrip,
    compact_top: CompactTop,
    command_bar: CommandBar,
    welcome: Option<Welcome>,
    welcome_show: bool,
    settings_open: bool,
    restore_view_after_settings: bool,
    settings_page: settings_page::SettingsPage,
    sidebar_width_override: Option<f32>,
    sidebar_resize_start: Option<f32>,
    passwords: Passwords,
    imports: Imports,
    form_rx: Option<Receiver<zeroize::Zeroizing<String>>>,
    form_origin: Option<String>,
    save_prompt: bool,
    password_menu_pos: Option<egui::Pos2>,
    password_menu_seen_focused: bool,
    grain: egui::TextureHandle,
    compact: Compact,
    saver: SessionSaver,
    saving_disabled: bool,
    started: Instant,
    last_frame_start: Option<Instant>,
    initial: Vec<Command>,
    deferred_views: Vec<TabId>,
    test_page_pending: bool,
    notice: Option<String>,
    saved_notice_until: Option<Instant>,
    window_material: Option<(WindowMaterial, bool, Glass)>,
    glass_supported: Option<bool>,
    rounded_rect: Option<egui::Rect>,
    rounded_scale: Option<f64>,
    rounded_tab: Option<TabId>,
    rounded_radius: Option<f32>,
    text_scale: u16,
    smooth_scrolling: bool,
    visuals_dark: Option<bool>,
    visuals_opacity: u8,
    accent: egui::Color32,
    monitor_position: Option<(i32, i32)>,
    #[cfg(debug_assertions)]
    frame_count: Option<u64>,
    #[cfg(debug_assertions)]
    debug_shot: Option<crate::debug_shot::DebugShot>,
}

fn writable_data_dir(path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| error.to_string())?;
    let probe = path.join(format!(".aegis-write-probe-{}", std::process::id()));
    let file = std::fs::File::create(&probe).map_err(|error| error.to_string())?;
    let synced = file.sync_all();
    drop(file);
    let removed = std::fs::remove_file(probe);
    synced.map_err(|error| error.to_string())?;
    removed.map_err(|error| error.to_string())
}

fn frame_sleep(since_last: Duration, interval: Option<Duration>) -> Duration {
    interval.unwrap_or_default().saturating_sub(since_last)
}

fn load_protection(ctx: egui::Context) -> ProtectionLoad {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let lists = Shield::load_default().map_err(|error| format!("{error:?}"))?;
            let shield = Shield::from_loaded(&lists);
            let json = if cfg!(target_os = "windows") {
                Vec::new()
            } else {
                content_blocker_json(&lists.as_strs()).map_err(|error| format!("{error:?}"))?
            };
            Ok((shield, json))
        })();
        let _ = sender.send(result);
        ctx.request_repaint();
    });
    receiver
}

impl AegisApp {
    pub fn new(start: Option<Start>, data_dir: PathBuf, ctx: egui::Context) -> Self {
        favicons::init(&data_dir);
        ctx.set_theme(egui::ThemePreference::System);
        let directory_error = writable_data_dir(&data_dir).err();
        let session_path = data_dir.join("session.json");
        let (browser, notice, missing, saving_disabled) = match session::load(&session_path) {
            Ok(loaded) => {
                let notice = if loaded.source == session::Source::Backup {
                    Some(if loaded.main_move_failed {
                        "Restored session from backup. Saving is off until restart so the unreadable session isn't overwritten."
                            .to_owned()
                    } else if let Some(path) = &loaded.main_moved_to {
                        format!(
                            "Restored session from backup. Unreadable session kept at {}.",
                            path.file_name()
                                .unwrap_or(path.as_os_str())
                                .to_string_lossy()
                        )
                    } else {
                        "Restored session from backup".to_owned()
                    })
                } else {
                    None
                };
                (loaded.browser, notice, false, loaded.main_move_failed)
            }
            Err(failure) => {
                if matches!(failure.error, session::SessionError::Missing) {
                    (Browser::new(), None, true, false)
                } else {
                    let notice = if failure.move_failed {
                        "Couldn't restore your last session. Saving is off until restart so it isn't overwritten."
                            .to_owned()
                    } else if let Some(path) = &failure.moved_to {
                        format!(
                            "Couldn't restore your last session. It was kept at {}.",
                            path.file_name()
                                .unwrap_or(path.as_os_str())
                                .to_string_lossy()
                        )
                    } else {
                        format!("Session restore: {}", failure.error)
                    };
                    (Browser::new(), Some(notice), true, failure.move_failed)
                }
            }
        };
        let mut initial = Vec::new();
        if missing {
            if let Ok(url) = aegis_ipc::page_url(command_bar::search_engine_home_url(
                browser.settings().search_engine,
            )) {
                initial.push(Command::OpenTab {
                    url: url.into_string(),
                    placement: Placement::End,
                    activate: true,
                    opener: None,
                });
            }
        } else if let Some(tab) = browser.active_tab() {
            initial.push(Command::ActivateTab(tab));
        }
        let mut test_page_pending = false;
        if let Some(start) = start {
            let url = match start {
                Start::Url(url) => url,
                Start::Html(_) => {
                    test_page_pending = true;
                    "about:blank".to_owned()
                }
            };
            if let Ok(url) = aegis_ipc::page_url(&url) {
                initial.push(Command::OpenTab {
                    url: url.into_string(),
                    placement: Placement::AfterActive,
                    activate: true,
                    opener: None,
                });
            }
        }
        crate::motion::set_frame_limit(
            browser.settings().frame_limit,
            browser.settings().custom_fps,
        );
        fonts::install(&ctx, browser.settings().text_scale);
        let text_scale = browser.settings().text_scale;
        let smooth_scrolling = browser.settings().smooth_scrolling;
        theme::apply_scroll_animation(&ctx, smooth_scrolling);
        let accent = theme::accent_color(browser.settings());
        fonts::set_accent(&ctx, accent);
        let grain = glass::load_noise(&ctx);
        // AEGIS_DEBUG_WELCOME=1 shows welcome again in debug builds
        let welcome_show = crate::welcome::should_show(
            browser.settings().welcome_done,
            cfg!(debug_assertions) && std::env::var("AEGIS_DEBUG_WELCOME").as_deref() == Ok("1"),
        );
        let debug_settings = if cfg!(debug_assertions) {
            std::env::var("AEGIS_DEBUG_SETTINGS")
                .ok()
                .and_then(|value| settings_page::Page::from_debug(&value))
        } else {
            None
        };
        let welcome_show = welcome_show && debug_settings.is_none();
        let views = Views::new(
            data_dir.clone(),
            browser.settings().https_only,
            browser.settings().block_trackers,
        );
        let loader = directory_error
            .is_none()
            .then(|| load_protection(ctx.clone()));
        let engine = directory_error.is_none().then(|| {
            let wake = ctx.clone();
            Engine::new(
                data_dir.clone(),
                Box::new(move || wake.request_repaint()),
                smooth_scrolling,
                browser.settings().hardware_acceleration,
            )
        });
        let vault_path = data_dir.join("vault.json");
        let passwords = Passwords::new(vault_path.clone())
            .unwrap_or_else(|error| Passwords::unavailable(vault_path, error.to_string()));
        Self {
            browser,
            views,
            engine,
            data_dir,
            directory_error,
            protection_loader: loader,
            tracker_loaded: false,
            tracker_missing: false,
            rule_fallback_attempted: false,
            sidebar: Sidebar::new(),
            tabstrip: TabStrip::new(),
            compact_top: CompactTop::new(),
            command_bar: CommandBar::new(),
            welcome: None,
            welcome_show,
            settings_open: debug_settings.is_some(),
            restore_view_after_settings: false,
            settings_page: settings_page::SettingsPage::with_page(
                debug_settings.unwrap_or_default(),
            ),
            sidebar_width_override: None,
            sidebar_resize_start: None,
            passwords,
            imports: Imports::new(),
            form_rx: None,
            form_origin: None,
            save_prompt: false,
            password_menu_pos: None,
            password_menu_seen_focused: false,
            grain,
            compact: Compact::new(),
            saver: SessionSaver::new(session_path),
            saving_disabled,
            started: Instant::now(),
            last_frame_start: None,
            initial,
            deferred_views: Vec::new(),
            test_page_pending,
            notice,
            saved_notice_until: None,
            window_material: None,
            glass_supported: None,
            rounded_rect: None,
            rounded_scale: None,
            rounded_tab: None,
            rounded_radius: None,
            text_scale,
            smooth_scrolling,
            visuals_dark: None,
            visuals_opacity: 0,
            accent,
            monitor_position: None,
            #[cfg(debug_assertions)]
            frame_count: (std::env::var("AEGIS_DEBUG_FRAMES").as_deref() == Ok("1")).then_some(0),
            #[cfg(debug_assertions)]
            debug_shot: crate::debug_shot::DebugShot::from_env(),
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn retry_data_dir(&mut self, ctx: &egui::Context) {
        if let Err(error) = writable_data_dir(&self.data_dir) {
            self.directory_error = Some(error);
            return;
        }
        self.directory_error = None;
        self.views = Views::new(
            self.data_dir.clone(),
            self.browser.settings().https_only,
            self.browser.settings().block_trackers,
        );
        let wake = ctx.clone();
        self.engine = Some(Engine::new(
            self.data_dir.clone(),
            Box::new(move || wake.request_repaint()),
            self.browser.settings().smooth_scrolling,
            self.browser.settings().hardware_acceleration,
        ));
        self.protection_loader = Some(load_protection(ctx.clone()));
        ctx.request_repaint();
    }

    fn poll_protection(&mut self, ctx: &egui::Context) {
        if let Some(error) = self.views.take_rule_error() {
            self.tracker_loaded = false;
            self.tracker_missing = true;
            if !self.rule_fallback_attempted {
                self.rule_fallback_attempted = true;
                if let Some(engine) = &self.engine
                    && let Err(fallback_error) = self.views.prepare(engine, None, Vec::new(), ctx)
                {
                    self.notice = Some(fallback_error);
                }
            } else {
                self.notice = Some(error);
            }
        }
        let Some(loader) = &self.protection_loader else {
            return;
        };
        let Ok(result) = loader.try_recv() else {
            return;
        };
        self.protection_loader = None;
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        let (shield, mut json) = match result {
            Ok((shield, json)) => {
                self.tracker_loaded = true;
                (Some(shield), json)
            }
            Err(_) => {
                self.tracker_missing = true;
                (None, Vec::new())
            }
        };
        if cfg!(debug_assertions) && !cfg!(target_os = "windows") {
            json.push(
                r#"[{"trigger":{"url-filter":"aegis-block"},"action":{"type":"block"}}]"#
                    .to_owned(),
            );
        }
        if let Err(error) = self.views.prepare(engine, shield, json, ctx) {
            self.notice = Some(error);
            self.tracker_loaded = false;
            self.tracker_missing = true;
            self.rule_fallback_attempted = true;
            let _ = self.views.prepare(engine, None, Vec::new(), ctx);
        }
        crate::protections::set_state(crate::protections::ProtectionState {
            lists_loaded: self.tracker_loaded && self.views.rules_ready(),
            block_trackers: self.views.blocking_reported(),
            https_only: self.browser.settings().https_only,
            vault_exists: self.passwords.state.exists(),
            vault_os_protected: self.passwords.state.os_protected(),
        });
    }

    fn set_settings_open(&mut self, open: bool) {
        if self.settings_open == open {
            return;
        }
        self.settings_open = open;
        if !open {
            self.restore_view_after_settings = true;
            return;
        }
        self.hide_settings_view();
    }

    fn hide_settings_view(&mut self) {
        if let Some(view) = self.views.active(&self.browser)
            && let Err(error) = view.set_visible(false)
        {
            self.notice = Some(error.to_string());
        }
    }

    fn apply(&mut self, command: Command, frame: &eframe::Frame, ctx: &egui::Context) {
        let is_test_open = self.test_page_pending
            && matches!(&command, Command::OpenTab { url, .. } if url == "about:blank");
        let stale_title = stale_title_tab(&self.browser, &command);
        let effects = match self.browser.apply(command, self.now_ms()) {
            Ok(effects) => effects,
            Err(error) => {
                self.notice = Some(error.to_string());
                return;
            }
        };
        if let Some(tab) = stale_title {
            let _ = self.browser.apply(
                Command::SetTitle {
                    tab,
                    title: String::new(),
                },
                self.now_ms(),
            );
        }
        crate::motion::set_frame_limit(
            self.browser.settings().frame_limit,
            self.browser.settings().custom_fps,
        );
        self.views
            .set_https_only(self.browser.settings().https_only);
        if let Some(error) = self
            .views
            .set_block_trackers(self.browser.settings().block_trackers)
        {
            self.notice = Some(error);
        }
        if is_test_open {
            if let Some(tab) = self.browser.active_tab() {
                self.views.set_test_page(tab);
            }
            self.test_page_pending = false;
        }
        for effect in effects {
            if effect == Effect::PersistSession {
                if !self.saving_disabled {
                    self.saver.mark_dirty(self.now_ms());
                }
                continue;
            }
            let created_tab = match &effect {
                Effect::CreateView { tab, .. } => Some(*tab),
                _ => None,
            };
            let Some(engine) = self.engine.as_mut() else {
                return;
            };
            match self.views.execute(
                effect,
                engine,
                frame,
                ctx,
                self.browser.active_tab(),
                self.settings_open,
            ) {
                Ok(ExecuteResult::Command(command)) => {
                    if let Err(error) = self.browser.apply(command, self.now_ms()) {
                        self.notice = Some(error.to_string());
                    }
                }
                Ok(ExecuteResult::Deferred(tab)) => {
                    if !self.deferred_views.contains(&tab) {
                        self.deferred_views.push(tab);
                    }
                }
                Ok(ExecuteResult::Done) => {}
                Err(error) => self.notice = Some(error),
            }
            if created_tab == self.browser.active_tab()
                && let Some(view) = self.views.active(&self.browser)
                && let Err(error) = aegis_ffi::set_page_corner_radius(
                    view.webview(),
                    if self.browser.settings().floating_page && !cfg!(target_os = "linux") {
                        f64::from(
                            theme::Metrics::from_settings(self.browser.settings()).pane_radius,
                        )
                    } else {
                        0.0
                    },
                )
            {
                self.notice = Some(error.to_string());
            }
        }
    }

    fn go_back(&mut self, frame: &eframe::Frame, ctx: &egui::Context) {
        let Some(tab) = self.browser.active_tab() else {
            return;
        };
        let Some(view) = self.views.active(&self.browser) else {
            return;
        };
        match view.go_back() {
            Ok(true) => {}
            Ok(false) => {
                let Some(opener) = self.browser.tab(tab).and_then(|tab| tab.opener()) else {
                    return;
                };
                if self.browser.tab(opener).is_none() {
                    return;
                }
                self.apply(Command::CloseTab(tab), frame, ctx);
                self.apply(Command::ActivateTab(opener), frame, ctx);
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
    }

    fn handle_shortcut(&mut self, shortcut: Shortcut, frame: &eframe::Frame, ctx: &egui::Context) {
        match shortcut {
            Shortcut::CommandBar => self.open_bar(Mode::CurrentTab),
            Shortcut::Settings => self.set_settings_open(true),
            Shortcut::LockVault => {
                self.lock_vault();
            }
            Shortcut::NewTab => self.open_bar(Mode::NewTab),
            Shortcut::CloseTab => {
                if let Some(tab) = self.browser.active_tab() {
                    self.apply(Command::CloseTab(tab), frame, ctx);
                }
            }
            Shortcut::NextTab | Shortcut::PrevTab => {
                let order = self.browser.visual_order(self.browser.active_workspace());
                if order.is_empty() {
                    return;
                }
                let current = self
                    .browser
                    .active_tab()
                    .and_then(|id| order.iter().position(|tab| *tab == id))
                    .unwrap_or(0);
                let next = if shortcut == Shortcut::NextTab {
                    (current + 1) % order.len()
                } else {
                    (current + order.len() - 1) % order.len()
                };
                self.apply(Command::ActivateTab(order[next]), frame, ctx);
            }
            Shortcut::Reload => {
                if let Some(view) = self.views.active(&self.browser)
                    && let Err(error) = view.reload()
                {
                    self.notice = Some(error.to_string());
                }
            }
            Shortcut::Back => self.go_back(frame, ctx),
            Shortcut::Forward => {
                if let Some(view) = self.views.active(&self.browser)
                    && let Err(error) = view.go_forward()
                {
                    self.notice = Some(error.to_string());
                }
            }
            Shortcut::ToggleCompact => self.action(
                Action::SetCompact(!self.browser.settings().compact),
                frame,
                ctx,
            ),
        }
    }

    fn open_bar(&mut self, mode: Mode) {
        let active_url = self
            .browser
            .active_tab()
            .and_then(|id| self.browser.tab(id))
            .map(|tab| tab.url());
        self.command_bar.open(mode, active_url);
    }

    fn keyboard(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.settings_open && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.set_settings_open(false);
            return;
        }
        let keys = ctx.input(|input| {
            let command = input.modifiers.command;
            let ctrl = input.modifiers.ctrl;
            let shift = input.modifiers.shift;
            let alt = input.modifiers.alt;
            let mut keys = Vec::new();
            if command && input.key_pressed(egui::Key::Comma) {
                keys.push(Shortcut::Settings);
            }
            if command && input.key_pressed(egui::Key::T) {
                keys.push(Shortcut::NewTab);
            }
            if command && !shift && input.key_pressed(egui::Key::L) {
                keys.push(Shortcut::CommandBar);
            }
            if command && shift && input.key_pressed(egui::Key::L) {
                keys.push(Shortcut::LockVault);
            }
            if command && input.key_pressed(egui::Key::W) {
                keys.push(Shortcut::CloseTab);
            }
            if command && shift && input.key_pressed(egui::Key::S) {
                keys.push(Shortcut::ToggleCompact);
            }
            if ctrl && input.key_pressed(egui::Key::Tab) {
                keys.push(if shift {
                    Shortcut::PrevTab
                } else {
                    Shortcut::NextTab
                });
            }
            if (command && input.key_pressed(egui::Key::R)) || input.key_pressed(egui::Key::F5) {
                keys.push(Shortcut::Reload);
            }
            if (alt && input.key_pressed(egui::Key::ArrowLeft))
                || (cfg!(target_os = "macos")
                    && command
                    && input.key_pressed(egui::Key::OpenBracket))
            {
                keys.push(Shortcut::Back);
            }
            if (alt && input.key_pressed(egui::Key::ArrowRight))
                || (cfg!(target_os = "macos")
                    && command
                    && input.key_pressed(egui::Key::CloseBracket))
            {
                keys.push(Shortcut::Forward);
            }
            keys
        });
        for shortcut in keys {
            self.handle_shortcut(shortcut, frame, ctx);
        }
        for shortcut in self.views.take_shortcuts() {
            self.handle_shortcut(shortcut, frame, ctx);
        }
    }

    fn action(&mut self, action: Action, frame: &eframe::Frame, ctx: &egui::Context) {
        match action {
            Action::Command(Command::MoveTab { tab, to }) => {
                let to = crate::sidebar::location_after_removal(&self.browser, tab, to);
                self.apply(Command::MoveTab { tab, to }, frame, ctx);
            }
            Action::Command(command) => self.apply(command, frame, ctx),
            Action::OpenBar(mode) => self.open_bar(mode),
            Action::Back => self.go_back(frame, ctx),
            Action::Forward => self.handle_shortcut(Shortcut::Forward, frame, ctx),
            Action::Reload => self.handle_shortcut(Shortcut::Reload, frame, ctx),
            Action::Stop => {
                if let Some(view) = self.views.active(&self.browser)
                    && let Err(error) = view.webview().evaluate_script("window.stop()")
                {
                    self.notice = Some(error.to_string());
                }
            }
            Action::DismissNotice => self.notice = None,
            Action::SetCompact(compact) => {
                self.apply(Command::SetCompact(compact), frame, ctx);
                self.compact.reset();
                self.compact_top.reset();
            }
            Action::OpenSettings => self.set_settings_open(true),
            Action::OpenPasswords => {
                self.passwords.menu_open = !self.passwords.menu_open;
                self.password_menu_pos = ctx.input(|input| input.pointer.latest_pos());
                self.password_menu_seen_focused = false;
            }
        }
    }

    fn run_bar_action(&mut self, action: BarAction, frame: &eframe::Frame, ctx: &egui::Context) {
        let BarAction {
            mode,
            suggestion,
            force_new_tab,
        } = action;
        let SuggestionAction::Open(url) = suggestion else {
            if let SuggestionAction::ActivateTab(tab) = suggestion {
                self.apply(Command::ActivateTab(tab), frame, ctx);
            }
            return;
        };
        match if force_new_tab { Mode::NewTab } else { mode } {
            Mode::NewTab => self.apply(
                Command::OpenTab {
                    url,
                    placement: Placement::AfterActive,
                    activate: true,
                    opener: None,
                },
                frame,
                ctx,
            ),
            Mode::CurrentTab => {
                if let Some(tab) = self.browser.active_tab() {
                    self.apply(Command::Navigate { tab, url }, frame, ctx);
                } else {
                    self.apply(
                        Command::OpenTab {
                            url,
                            placement: Placement::End,
                            activate: true,
                            opener: None,
                        },
                        frame,
                        ctx,
                    );
                }
            }
        }
    }

    fn save_due(&mut self) {
        if self.saving_disabled {
            return;
        }
        if let Err(error) = self.saver.save_if_due(&self.browser, self.now_ms()) {
            self.notice = Some(error.to_string());
            self.saver.mark_dirty(self.now_ms());
        }
    }

    fn lock_vault(&mut self) {
        self.passwords.state.lock();
        self.passwords.clear_copied_password();
        self.passwords.revealed = None;
        self.passwords.clear_form();
        self.imports.clear_passwords();
        self.save_prompt = false;
        self.passwords.refresh_origin();
    }

    fn handle_password_action(
        &mut self,
        action: passwords::Action,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) {
        match action {
            passwords::Action::Gate(action) => self.passwords.request_gate(action, frame, ctx),
            passwords::Action::Save => {
                let now = self.now_ms();
                let result = if let Some(id) = self.passwords.edit {
                    self.passwords
                        .state
                        .update_password(id, &self.passwords.password, now)
                } else {
                    self.passwords.state.add(
                        &self.passwords.origin,
                        &self.passwords.username,
                        &self.passwords.password,
                        now,
                    )
                };
                match result {
                    Ok(()) => {
                        self.passwords.clear_form();
                        self.passwords.refresh_origin();
                        self.passwords.message = Some("Login saved".to_owned());
                    }
                    Err(error) => self.passwords.message = Some(error.to_string()),
                }
            }
            passwords::Action::Delete(id) => match self.passwords.state.remove(id) {
                Ok(()) => {
                    self.passwords.refresh_origin();
                    self.passwords.message = Some("Login deleted".to_owned());
                }
                Err(error) => self.passwords.message = Some(error.to_string()),
            },
            passwords::Action::SetMaster => {
                if !self.passwords.state.exists() {
                    self.passwords.master_change = false;
                } else if self.passwords.state.has_master() {
                    self.passwords.master_remove = true;
                } else {
                    self.passwords.master_change = true;
                }
                self.passwords.master_prompt = true;
            }
            passwords::Action::Lock => {
                self.lock_vault();
            }
            passwords::Action::Create => match self.passwords.state.create(None) {
                Ok(()) => {
                    self.passwords.refresh_origin();
                }
                Err(aegis_vault::VaultError::KeystoreUnavailable) => {
                    self.passwords.master_prompt = true
                }
                Err(error) => self.passwords.message = Some(error.to_string()),
            },
            passwords::Action::Unlock => {
                if let Err(error) = self.passwords.state.open_without_master() {
                    self.passwords.message = Some(error.to_string());
                }
                self.passwords.refresh_origin();
            }
            passwords::Action::Cancel => self.passwords.clear_form(),
        }
    }

    fn process_protected(&mut self, action: ProtectedAction) {
        match action {
            ProtectedAction::Reveal(id) => match self.passwords.state.reveal(id) {
                Ok(password) => {
                    self.passwords.revealed =
                        Some((id, password, Instant::now() + Duration::from_secs(30)))
                }
                Err(error) => self.passwords.message = Some(error.to_string()),
            },
            ProtectedAction::Copy(id) => match self.passwords.state.reveal(id) {
                Ok(password) => {
                    if let Err(error) = self.passwords.copy_password(&password) {
                        self.passwords.message = Some(error.to_string());
                    }
                }
                Err(error) => self.passwords.message = Some(error.to_string()),
            },
            ProtectedAction::Edit(id) => {
                let meta = self
                    .passwords
                    .state
                    .list()
                    .into_iter()
                    .find(|entry| entry.id == id);
                if let Some(meta) = meta {
                    match self.passwords.state.reveal(id) {
                        Ok(password) => {
                            self.passwords.origin = meta.origin;
                            self.passwords.username = meta.username;
                            self.passwords.password = password;
                            self.passwords.edit = Some(id);
                        }
                        Err(error) => self.passwords.message = Some(error.to_string()),
                    }
                }
            }
            ProtectedAction::Fill(id) => {
                let Some(meta) = self
                    .passwords
                    .state
                    .list()
                    .into_iter()
                    .find(|entry| entry.id == id)
                else {
                    return;
                };
                let Some(view) = self.views.active(&self.browser) else {
                    return;
                };
                let Ok(current_url) = view.webview().url() else {
                    return;
                };
                if !passwords::same_origin(&current_url, &meta.origin) {
                    self.passwords.message =
                        Some("The page changed; choose the login again".to_owned());
                    return;
                }
                let password = match self.passwords.state.reveal(id) {
                    Ok(password) => password,
                    Err(error) => {
                        self.passwords.message = Some(error.to_string());
                        return;
                    }
                };
                let script = match passwords::fill_script(&meta.origin, &meta.username, &password) {
                    Ok(script) => script,
                    Err(_) => return,
                };
                drop(password);
                if let Err(error) = view.webview().evaluate_script(script.as_str()) {
                    self.passwords.message = Some(error.to_string());
                }
                drop(script);
            }
            ProtectedAction::Update(id) => {
                match self.passwords.state.update_password(
                    id,
                    &self.passwords.password,
                    self.now_ms(),
                ) {
                    Ok(()) => {
                        self.passwords.clear_form();
                        self.save_prompt = false;
                        self.passwords.message = Some("Password updated".to_owned());
                    }
                    Err(error) => self.passwords.message = Some(error.to_string()),
                }
            }
            ProtectedAction::Import => {
                let Some(preview) = self.imports.preview.take() else {
                    return;
                };
                let imported = self.passwords.state.import(&preview.logins, self.now_ms());
                drop(preview);
                self.imports.mark_imported();
                self.passwords.refresh_origin();
                self.imports.message = Some(match imported {
                    Ok(count) => format!("Imported {count} logins"),
                    Err(error) => error.to_string(),
                });
            }
        }
    }

    fn handle_import_action(
        &mut self,
        action: imports::Action,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) {
        match action {
            imports::Action::Bookmarks(path, source) => {
                match imports::parse_bookmarks(&path, &source) {
                    Ok(bookmarks) => {
                        let skipped = bookmarks.skipped_invalid;
                        let plan = aegis_import::to_tab_plan(&bookmarks);
                        if let Some(view) = self.views.active(&self.browser) {
                            let _ = view.set_visible(false);
                        }
                        let now = self.now_ms();
                        match imports::apply_tab_plan(&mut self.browser, plan, &source, now) {
                            Ok((_, count)) => {
                                self.saver.mark_dirty(self.now_ms());
                                self.imports.message = Some(format!(
                                    "Imported {count} bookmarks, skipped {skipped} as invalid"
                                ));
                            }
                            Err(error) => self.imports.message = Some(error.to_string()),
                        }
                    }
                    Err(message) => self.imports.message = Some(message),
                }
            }
            imports::Action::Passwords => {
                self.passwords
                    .request_gate(ProtectedAction::Import, frame, ctx)
            }
            imports::Action::Cancel => self.imports.clear_preview(),
            imports::Action::DeleteCsv => match self.imports.delete_csv() {
                Ok(()) => self.imports.message = Some("CSV deleted".to_owned()),
                Err(imports::DeleteCsvError::Changed) => {
                    self.imports.message = Some(
                        "The file changed since it was imported, so it wasn't deleted.".to_owned(),
                    )
                }
                Err(imports::DeleteCsvError::Io(error)) => {
                    self.imports.message = Some(format!("Couldn't delete the CSV file: {error}"));
                }
            },
        }
    }

    fn begin_save_from_page(&mut self, ctx: &egui::Context) {
        let Some(view) = self.views.active(&self.browser) else {
            return;
        };
        let Ok(url) = view.webview().url() else {
            return;
        };
        let Some(origin) = passwords::https_origin(&url) else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        match view.eval_with_result(passwords::READ_FORM_SCRIPT, move |result| {
            let _ = sender.send(zeroize::Zeroizing::new(result));
            wake.request_repaint();
        }) {
            Ok(()) => {
                self.form_rx = Some(receiver);
                self.form_origin = Some(origin);
            }
            Err(error) => self.passwords.message = Some(error.to_string()),
        }
    }

    fn poll_form_result(&mut self) {
        let Some(receiver) = &self.form_rx else {
            return;
        };
        let Ok(raw) = receiver.try_recv() else {
            return;
        };
        self.form_rx = None;
        let Some(origin) = self.form_origin.take() else {
            return;
        };
        let Some(view) = self.views.active(&self.browser) else {
            return;
        };
        let Ok(current) = view.webview().url() else {
            return;
        };
        if !passwords::same_origin(&current, &origin) {
            self.passwords.message = Some("The page changed; try saving again".to_owned());
            return;
        }
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            self.passwords.message = Some("Could not read this form".to_owned());
            return;
        };
        let Some(object) = value.as_object_mut() else {
            self.passwords.message = Some("No visible password field was found".to_owned());
            return;
        };
        let Some(serde_json::Value::String(password)) = object.remove("password") else {
            self.passwords.message = Some("No visible password field was found".to_owned());
            return;
        };
        let username = match object.remove("username") {
            Some(serde_json::Value::String(value)) => value,
            _ => String::new(),
        };
        self.passwords.origin = origin;
        self.passwords.username = username;
        self.passwords.password = zeroize::Zeroizing::new(password);
        // page fields are untrusted; the user confirms both values in the native prompt
        self.save_prompt = true;
    }

    fn show_password_menu(&mut self, ctx: &egui::Context) -> Option<ProtectedAction> {
        if !self.passwords.menu_open {
            return None;
        }
        let origin = self.passwords.active_origin()?.to_owned();
        let entries = self.passwords.state.for_origin(&origin);
        let pos = self.password_menu_pos.unwrap_or(egui::pos2(100.0, 100.0));
        let window = ctx
            .input(|input| input.viewport().outer_rect)
            .map_or(egui::Pos2::ZERO, |rect| rect.min);
        let builder = egui::ViewportBuilder::default()
            .with_icon(crate::logo::icon())
            .with_position(window + pos.to_vec2())
            .with_inner_size([260.0, 40.0 + 32.0 * entries.len() as f32 + 38.0])
            .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
            .with_has_shadow(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(true);
        let (choice, save, focused) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("password_menu"),
            builder,
            |ctx, _| {
                let mut choice = None;
                let mut save = false;
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Saved logins");
                    for entry in &entries {
                        if ui.button(&entry.username).clicked() {
                            choice = Some(ProtectedAction::Fill(entry.id));
                        }
                    }
                    ui.separator();
                    if ui.button("Save login for this page…").clicked() {
                        save = true;
                    }
                });
                (choice, save, ctx.input(|input| input.focused))
            },
        );
        self.password_menu_seen_focused |= focused;
        if choice.is_some() || save || (self.password_menu_seen_focused && !focused) {
            self.passwords.menu_open = false;
        }
        if save {
            self.begin_save_from_page(ctx);
        }
        choice
    }

    fn show_save_prompt(&mut self, ctx: &egui::Context) -> Option<passwords::Action> {
        if !self.save_prompt {
            return None;
        }
        let existing_for_label = self
            .passwords
            .state
            .for_origin(&self.passwords.origin)
            .into_iter()
            .find(|meta| meta.username == self.passwords.username)
            .map(|meta| meta.id);
        let builder = egui::ViewportBuilder::default()
            .with_icon(crate::logo::icon())
            .with_inner_size([390.0, 210.0])
            .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
            .with_has_shadow(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(true);
        let mut save = false;
        let mut cancel = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("save_login_prompt"),
            builder,
            |ctx, _| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Save login for this page");
                    ui.label(&self.passwords.origin);
                    ui.horizontal(|ui| {
                        ui.label("Username");
                        ui.text_edit_singleline(&mut self.passwords.username);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Password");
                        ui.add(
                            egui::TextEdit::singleline(&mut *self.passwords.password)
                                .password(!self.passwords.show_password),
                        );
                        ui.checkbox(&mut self.passwords.show_password, "Show");
                    });
                    ui.label("Only visible fields in the top frame are read; iframe forms are not included.");
                    if ui
                        .button(if existing_for_label.is_some() {
                            "Update password"
                        } else {
                            "Save"
                        })
                        .clicked()
                    {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            },
        );
        if cancel {
            self.save_prompt = false;
            self.passwords.clear_form();
        }
        if save {
            let existing = self
                .passwords
                .state
                .for_origin(&self.passwords.origin)
                .into_iter()
                .find(|meta| meta.username == self.passwords.username)
                .map(|meta| meta.id);
            if let Some(id) = existing {
                self.save_prompt = false;
                return Some(passwords::Action::Gate(ProtectedAction::Update(id)));
            }
            self.save_prompt = false;
            return Some(passwords::Action::Save);
        }
        None
    }

    #[cfg(debug_assertions)]
    fn count_frame(&mut self) {
        let Some(count) = &mut self.frame_count else {
            return;
        };
        *count += 1;
        eprintln!("frame {count}");
    }
}

impl eframe::App for AegisApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // overlays can halve the rate with vsync since each window waits for it
        if let Some(last) = self.last_frame_start {
            let delay = frame_sleep(last.elapsed(), crate::motion::current_frame_interval());
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
        }
        self.last_frame_start = Some(Instant::now());
        let ctx = ui.ctx().clone();
        if self.passwords.tick(&ctx) {
            self.imports.clear_passwords();
        }
        if let Some(action) = self.passwords.poll_gate() {
            self.process_protected(action);
        }
        if self
            .passwords
            .show_master_prompt(&ctx, self.browser.settings())
        {
            match self.passwords.submit_master() {
                Ok(Some(action)) => {
                    if self.passwords.master_as_gate {
                        self.passwords.master_as_gate = false;
                        self.process_protected(action);
                    } else {
                        self.passwords.request_gate(action, frame, &ctx);
                    }
                }
                Ok(None) => {}
                Err(error) => self.passwords.message = Some(error.to_string()),
            }
        }
        let preference = match self.browser.settings().theme {
            ThemeMode::System => egui::ThemePreference::System,
            ThemeMode::Light => egui::ThemePreference::Light,
            ThemeMode::Dark => egui::ThemePreference::Dark,
        };
        ctx.set_theme(preference);
        if self.text_scale != self.browser.settings().text_scale {
            self.text_scale = self.browser.settings().text_scale;
            fonts::apply_scale(&ctx, self.text_scale);
        }
        let smooth_scrolling = self.browser.settings().smooth_scrolling;
        if self.smooth_scrolling != smooth_scrolling {
            self.smooth_scrolling = smooth_scrolling;
            theme::apply_scroll_animation(&ctx, smooth_scrolling);
            if let Some(engine) = self.engine.as_mut() {
                engine.set_smooth_scrolling(smooth_scrolling);
            }
            self.views.set_smooth_scrolling(smooth_scrolling);
        }
        let appearance = theme::resolve(
            self.browser.settings(),
            ctx.system_theme() == Some(egui::Theme::Dark),
        );
        if self.visuals_dark != Some(appearance.dark)
            || self.visuals_opacity != self.browser.settings().sidebar_opacity
            || self.accent != appearance.accent
        {
            theme::apply_visuals(&ctx, appearance);
            self.visuals_dark = Some(appearance.dark);
            self.visuals_opacity = self.browser.settings().sidebar_opacity;
        }
        if self.accent != appearance.accent {
            self.accent = appearance.accent;
            fonts::set_accent(&ctx, self.accent);
        }
        let dark = appearance.dark;
        if let Some(window) = frame.winit_window() {
            if let Ok(position) = window.outer_position() {
                let position = (position.x, position.y);
                if self.monitor_position != Some(position) {
                    self.monitor_position = Some(position);
                    crate::motion::set_refresh_rate(
                        window
                            .current_monitor()
                            .and_then(|monitor| monitor.refresh_rate_millihertz()),
                    );
                }
            }
            let glass = *self
                .glass_supported
                .get_or_insert_with(|| aegis_ffi::window_glass_supported(window.as_ref()));
            let material = if glass && self.browser.settings().glass != Glass::Solid {
                WindowMaterial::Glass
            } else {
                WindowMaterial::Solid
            };
            if self.window_material != Some((material, dark, self.browser.settings().glass)) {
                match aegis_ffi::apply_window_material(window.as_ref(), material, dark) {
                    Ok(()) => {
                        self.window_material = Some((material, dark, self.browser.settings().glass))
                    }
                    Err(error) => self.notice = Some(error.to_string()),
                }
            }
        }
        let space_color = self
            .browser
            .workspace(self.browser.active_workspace())
            .map_or(GroupColor::Grey, |space| space.color());
        glass::paint(
            ui,
            self.browser.settings(),
            appearance,
            space_color,
            self.glass_supported == Some(true),
            &self.grain,
        );
        #[cfg(debug_assertions)]
        self.count_frame();
        #[cfg(debug_assertions)]
        if let Some(shot) = &mut self.debug_shot {
            shot.tick(&ctx, self.welcome_show);
        }
        if let Some(error) = &self.directory_error {
            let mut retry = false;
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() - 190.0).max(0.0) * 0.5);
                egui::Frame::new()
                    .fill(appearance.palette.overlay)
                    .corner_radius(appearance.metrics.overlay_radius)
                    .inner_margin(24.0)
                    .show(ui, |ui| {
                        ui.set_min_width(420.0);
                        ui.heading("Aegis can't save its data");
                        ui.label(
                            egui::RichText::new(self.data_dir.display().to_string())
                                .color(appearance.palette.secondary),
                        );
                        ui.label(egui::RichText::new(error).color(appearance.palette.secondary));
                        ui.add_space(12.0);
                        retry = ui.button("Try again").clicked();
                    });
            });
            if retry {
                self.retry_data_dir(&ctx);
            }
            return;
        }
        self.poll_protection(&ctx);
        if let Some(until) = self.saved_notice_until {
            if Instant::now() >= until {
                if self.notice.as_deref() == Some("Saved to Downloads") {
                    self.notice = None;
                }
                self.saved_notice_until = None;
            } else {
                ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
            }
        }
        self.views
            .set_https_only(self.browser.settings().https_only);
        crate::protections::set_state(crate::protections::ProtectionState {
            lists_loaded: self.tracker_loaded && self.views.rules_ready(),
            block_trackers: self.views.blocking_reported(),
            https_only: self.browser.settings().https_only,
            vault_exists: self.passwords.state.exists(),
            vault_os_protected: self.passwords.state.os_protected(),
        });
        let deferred = std::mem::take(&mut self.deferred_views);
        #[cfg(target_os = "linux")]
        {
            // webkitgtk needs its main loop pumped alongside winit
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if self.welcome_show {
            let (settings, finished) = self.welcome.get_or_insert_with(|| Welcome::new(&ctx)).show(
                ui,
                self.browser.settings(),
                appearance,
                space_color,
                self.glass_supported == Some(true),
                &self.grain,
            );
            if let Some(settings) = settings {
                if settings.layout != self.browser.settings().layout {
                    self.apply(Command::SetLayout(settings.layout), frame, &ctx);
                }
                if finished || settings != *self.browser.settings() {
                    self.apply(Command::SetSettings(settings), frame, &ctx);
                }
            }
            if finished {
                crate::welcome::finish(&mut self.welcome, &ctx);
                self.welcome_show = false;
                #[cfg(debug_assertions)]
                if let Some(shot) = &mut self.debug_shot {
                    shot.welcome_finished(&ctx);
                }
                ctx.request_repaint();
            }
            self.save_due();
            return;
        }
        self.keyboard(&ctx, frame);
        let (commands, notices) = if let Some(engine) = &self.engine {
            self.views
                .drain_events(engine, &self.browser, self.now_ms(), &ctx)
        } else {
            (Vec::new(), Vec::new())
        };
        for notice in notices {
            self.notice = Some(notice);
        }
        for command in commands {
            self.apply(command, frame, &ctx);
        }
        if self.settings_open {
            self.deferred_views.extend(deferred);
            let (command, close, clear_exceptions, clear_icons, password_actions, import_actions) =
                settings_page::show(
                    ui,
                    ui.max_rect(),
                    self.browser.settings(),
                    appearance,
                    &mut self.settings_page,
                    &mut self.passwords,
                    &mut self.imports,
                );
            for action in password_actions {
                self.handle_password_action(action, frame, &ctx);
            }
            for action in import_actions {
                self.handle_import_action(action, frame, &ctx);
            }
            if let Some(command) = command {
                self.apply(command, frame, &ctx);
            }
            if close {
                self.set_settings_open(false);
            }
            if clear_exceptions && let Err(error) = self.views.clear_exceptions() {
                self.notice = Some(error);
            }
            if clear_icons {
                favicons::clear();
            }
            self.save_due();
            if let Some(delay) = self.saver.time_until_due(self.now_ms()) {
                ctx.request_repaint_after(delay);
            }
            return;
        }
        let active_url = self
            .browser
            .active_tab()
            .and_then(|id| self.browser.tab(id))
            .map(|tab| tab.url());
        self.passwords.update_origin(active_url);
        let mut actions = Vec::new();
        let mut strip_hovered = false;
        let window_rect = ui.max_rect();
        let right = self.browser.settings().sidebar_side == Side::Right;
        let classic = self.browser.settings().layout == Layout::Horizontal;
        if classic || self.browser.settings().compact {
            self.sidebar_width_override = None;
            self.sidebar_resize_start = None;
        }
        let sidebar_width = self
            .sidebar_width_override
            .unwrap_or(appearance.metrics.sidebar_width);
        let panes = pane_layout_at_scale(
            window_rect,
            sidebar_width,
            self.browser.settings().sidebar_side,
            ctx.pixels_per_point(),
        );
        if classic && self.browser.settings().compact {
            egui::Panel::top("classic_hot_strip")
                .resizable(false)
                .exact_size(theme::HOT_STRIP_WIDTH)
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show(ui, |ui| {
                    strip_hovered = self.compact_top.hot_strip(ui);
                });
        } else if classic {
            egui::Panel::top("classic_tabs")
                .resizable(false)
                .exact_size(76.0)
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show(ui, |ui| {
                    actions.extend(self.tabstrip.show(
                        ui,
                        &self.browser,
                        &mut self.sidebar,
                        self.views.active_loading(&self.browser),
                        self.views.active_blocked(&self.browser),
                    ));
                });
        } else if self.browser.settings().compact {
            let panel = if right {
                egui::Panel::right("hot_strip")
            } else {
                egui::Panel::left("hot_strip")
            };
            panel
                .resizable(false)
                .exact_size(theme::HOT_STRIP_WIDTH)
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show(ui, |ui| {
                    strip_hovered = self.compact.hot_strip(ui);
                });
        } else {
            egui::Area::new(egui::Id::unique("sidebar_pane"))
                .fixed_pos(panes.sidebar_rect.min)
                .show(&ctx, |ui| {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(panes.sidebar_rect), |ui| {
                        actions.extend(self.sidebar.show(
                            ui,
                            &self.browser,
                            self.views.active_loading(&self.browser),
                            self.notice.as_deref(),
                            self.views.active_blocked(&self.browser),
                            right,
                        ));
                    });
                });
            let handle = panes.handle_rect.expand2(egui::vec2(1.0, 0.0));
            let response = ui.interact(
                handle,
                ui.scope_id().with("sidebar_resize"),
                egui::Sense::click_and_drag(),
            );
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                ui.painter().line_segment(
                    [
                        panes.handle_rect.center_top(),
                        panes.handle_rect.center_bottom(),
                    ],
                    egui::Stroke::new(
                        1.0,
                        appearance.accent.gamma_multiply(if response.dragged() {
                            0.8
                        } else {
                            0.4
                        }),
                    ),
                );
            }
            if response.double_clicked() {
                let width = match self.browser.settings().density {
                    aegis_core::Density::Compact => 248,
                    aegis_core::Density::Comfortable => 272,
                    aegis_core::Density::Spacious => 296,
                };
                let mut next = self.browser.settings().clone();
                next.sidebar_width = width;
                next.setup = aegis_core::Setup::Custom;
                self.sidebar_width_override = None;
                self.sidebar_resize_start = None;
                self.apply(Command::SetSettings(next), frame, &ctx);
            } else {
                if response.drag_started() {
                    self.sidebar_resize_start = Some(sidebar_width);
                }
                if response.dragged() {
                    let start = self.sidebar_resize_start.unwrap_or(sidebar_width);
                    let direction = if right { -1.0 } else { 1.0 };
                    self.sidebar_width_override =
                        Some((start + direction * response.drag_delta().x).clamp(220.0, 360.0));
                }
                if response.drag_stopped() {
                    if let Some(width) = self.sidebar_width_override.take() {
                        let mut next = self.browser.settings().clone();
                        next.sidebar_width = (((width.round() as u16 + 2) / 4) * 4).clamp(220, 360);
                        next.setup = aegis_core::Setup::Custom;
                        self.apply(Command::SetSettings(next), frame, &ctx);
                    }
                    self.sidebar_resize_start = None;
                }
            }
        }
        let full_content = egui::CentralPanel::no_frame()
            .show(ui, |ui| ui.max_rect())
            .inner;
        let content = if self.browser.settings().floating_page {
            if !classic && !self.browser.settings().compact {
                panes.page_rect
            } else if classic {
                classic_page_layout(full_content, ctx.pixels_per_point())
            } else {
                full_content.shrink(8.0)
            }
        } else if !classic && !self.browser.settings().compact {
            if right {
                egui::Rect::from_min_max(
                    window_rect.min,
                    egui::pos2(panes.handle_rect.left(), window_rect.bottom()),
                )
            } else {
                egui::Rect::from_min_max(
                    egui::pos2(panes.handle_rect.right(), window_rect.top()),
                    window_rect.max,
                )
            }
        } else {
            full_content
        };
        let radius = if self.browser.settings().floating_page && !cfg!(target_os = "linux") {
            appearance.metrics.pane_radius
        } else {
            0.0
        };
        if self.browser.settings().floating_page {
            let shadow = egui::Shadow {
                offset: [0, 12],
                blur: 40,
                spread: 0,
                color: if dark {
                    egui::Color32::from_black_alpha(115)
                } else {
                    egui::Color32::from_black_alpha(41)
                },
            };
            ui.painter().add(shadow.as_shape(content, radius));
            ui.painter()
                .rect_filled(content, radius, appearance.palette.page);
            ui.painter().rect_stroke(
                content,
                radius,
                egui::Stroke::new(theme::hairline(&ctx), appearance.palette.pane_edge),
                egui::StrokeKind::Inside,
            );
        } else {
            ui.painter()
                .rect_filled(content, 0.0, appearance.palette.page);
        }
        if let Err(error) = self.views.set_bounds(&ctx, frame, content) {
            self.notice = Some(error);
        }
        if self.restore_view_after_settings {
            self.restore_view_after_settings = false;
            if !self.views.active_hidden(&self.browser)
                && let Some(view) = self.views.active(&self.browser)
                && let Err(error) = view.set_visible(true)
            {
                self.notice = Some(error.to_string());
            }
        }
        if !self.views.rules_ready() {
            egui::Area::new(egui::Id::unique("preparing_protection"))
                .fixed_pos(content.center() - egui::vec2(90.0, 8.0))
                .show(&ctx, |ui| {
                    ui.label(
                        egui::RichText::new(crate::protections::PREPARING_LINE)
                            .size(11.0)
                            .color(appearance.palette.secondary),
                    );
                });
        }
        if self.tracker_missing {
            let window = ctx
                .input(|input| input.viewport().inner_rect)
                .unwrap_or(content);
            let builder = egui::ViewportBuilder::default()
                .with_icon(crate::logo::icon())
                .with_position(egui::pos2(
                    window.min.x + content.min.x + 16.0,
                    window.min.y + content.min.y + 12.0,
                ))
                .with_inner_size([295.0, 28.0])
                .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
                .with_has_shadow(false)
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_always_on_top()
                .with_active(false);
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("missing_tracker_lists"),
                builder,
                |ctx, _| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            egui::Frame::new()
                                .fill(appearance.palette.overlay)
                                .corner_radius(appearance.metrics.control_radius)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            crate::protections::MISSING_LISTS_NOTICE,
                                        )
                                        .size(11.0)
                                        .color(appearance.palette.secondary),
                                    );
                                });
                        });
                },
            );
        }
        if self.notice.as_deref() == Some("Saved to Downloads") {
            let window = ctx
                .input(|input| input.viewport().inner_rect)
                .unwrap_or(content);
            let builder = egui::ViewportBuilder::default()
                .with_icon(crate::logo::icon())
                .with_position(egui::pos2(
                    window.min.x + content.max.x - 180.0,
                    window.min.y + content.min.y + 12.0,
                ))
                .with_inner_size([164.0, 28.0])
                .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
                .with_has_shadow(false)
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_always_on_top()
                .with_active(false);
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("download_saved_notice"),
                builder,
                |ctx, _| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            egui::Frame::new()
                                .fill(appearance.palette.overlay)
                                .corner_radius(appearance.metrics.control_radius)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.label("Saved to Downloads");
                                });
                        });
                },
            );
        }
        let active = self.browser.active_tab();
        let scale = frame.winit_window().map(|window| window.scale_factor());
        if (self.rounded_rect != Some(content)
            || self.rounded_scale != scale
            || self.rounded_tab != active
            || self.rounded_radius != Some(radius))
            && let Some(view) = self.views.active(&self.browser)
        {
            match aegis_ffi::set_page_corner_radius(view.webview(), f64::from(radius)) {
                Ok(()) => {
                    self.rounded_rect = Some(content);
                    self.rounded_scale = scale;
                    self.rounded_tab = active;
                    self.rounded_radius = Some(radius);
                }
                Err(error) => self.notice = Some(error.to_string()),
            }
        }
        for tab in deferred {
            if self.browser.active_tab() == Some(tab) && !self.views.has_view(tab) {
                self.apply(Command::ActivateTab(tab), frame, &ctx);
            }
        }
        for command in std::mem::take(&mut self.initial) {
            self.apply(command, frame, &ctx);
        }
        if let Some(window) = ctx.input(|input| input.viewport().inner_rect) {
            if classic && self.browser.settings().compact {
                actions.extend(self.compact_top.show(
                    &ctx,
                    window,
                    strip_hovered,
                    &mut self.tabstrip,
                    &self.browser,
                    &mut self.sidebar,
                    self.views.active_loading(&self.browser),
                    self.views.active_blocked(&self.browser),
                ));
            } else if self.browser.settings().compact {
                actions.extend(self.compact.show(
                    &ctx,
                    window,
                    strip_hovered,
                    &mut self.sidebar,
                    &self.browser,
                    self.views.active_loading(&self.browser),
                    self.notice.as_deref(),
                    self.views.active_blocked(&self.browser),
                    right,
                    appearance.metrics.sidebar_width,
                ));
            }
            if let Some(action) = self.command_bar.show(&ctx, content, window, &self.browser) {
                self.run_bar_action(action, frame, &ctx);
            }
        }
        for action in actions {
            self.action(action, frame, &ctx);
        }
        let active_url = self
            .browser
            .active_tab()
            .and_then(|id| self.browser.tab(id))
            .map(|tab| tab.url());
        self.passwords.update_origin(active_url);
        self.poll_form_result();
        if let Some(action) = self.show_password_menu(&ctx) {
            self.passwords.request_gate(action, frame, &ctx);
        }
        if let Some(action) = self.show_save_prompt(&ctx) {
            self.handle_password_action(action, frame, &ctx);
        }
        if !self.settings_open {
            let (overlay, notice) =
                self.views
                    .show_overlays(&ctx, content, appearance, self.browser.active_tab());
            if let Some(notice) = notice {
                if notice == "Saved to Downloads" {
                    self.saved_notice_until = Some(Instant::now() + Duration::from_secs(3));
                }
                self.notice = Some(notice);
            }
            if let Some(overlay) = overlay {
                match overlay {
                    OverlayAction::GoBack(tab) => {
                        if let Some(view) = self.views.active(&self.browser) {
                            let _ = view.set_visible(true);
                            match view.go_back() {
                                Ok(true) => {}
                                Ok(false) => self.apply(
                                    Command::Navigate {
                                        tab,
                                        url: "about:blank".to_owned(),
                                    },
                                    frame,
                                    &ctx,
                                ),
                                Err(error) => self.notice = Some(error.to_string()),
                            }
                        }
                    }
                    OverlayAction::Navigate(tab, url) => {
                        self.apply(Command::Navigate { tab, url }, frame, &ctx);
                        if let Some(view) = self.views.active(&self.browser) {
                            let _ = view.set_visible(true);
                        }
                    }
                    OverlayAction::Reload(tab) => {
                        if let Some(engine) = self.engine.as_mut() {
                            let _ = self.views.execute(
                                Effect::DestroyView(tab),
                                engine,
                                frame,
                                &ctx,
                                self.browser.active_tab(),
                                false,
                            );
                        }
                        self.apply(
                            Command::SetLoadState {
                                tab,
                                state: LoadState::Discarded,
                            },
                            frame,
                            &ctx,
                        );
                        self.apply(Command::ActivateTab(tab), frame, &ctx);
                    }
                }
            }
        }
        if self.passwords.tick(&ctx) {
            self.imports.clear_passwords();
        }
        self.save_due();
        if let Some(delay) = self.saver.time_until_due(self.now_ms()) {
            ctx.request_repaint_after(delay);
        }
        if !self.deferred_views.is_empty() && self.views.rules_ready() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.lock_vault();
        if self.saving_disabled || self.directory_error.is_some() {
            return;
        }
        if let Err(error) = self.saver.save(&self.browser) {
            eprintln!("session save: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_host_navigation_replaces_old_title_with_host() {
        let mut browser = Browser::new();
        let _ = browser.apply(
            Command::OpenTab {
                url: "https://google.com/search?q=ivr".to_owned(),
                placement: Placement::End,
                activate: true,
                opener: None,
            },
            0,
        );
        let Some(tab) = browser.active_tab() else {
            panic!("tab did not open")
        };
        let _ = browser.apply(
            Command::SetTitle {
                tab,
                title: "ivr - Google Search".to_owned(),
            },
            0,
        );
        let command = Command::Navigate {
            tab,
            url: "https://illinivexu.web.illinois.edu/".to_owned(),
        };
        assert_eq!(stale_title_tab(&browser, &command), Some(tab));
        let _ = browser.apply(command, 0);
        let _ = browser.apply(
            Command::SetTitle {
                tab,
                title: String::new(),
            },
            0,
        );
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

    #[test]
    fn pane_gaps_match_in_physical_pixels() {
        for (width, height) in [(640.0, 480.0), (1280.0, 800.0), (2000.0, 1067.0)] {
            for scale in [1.0_f32, 1.25, 1.5, 2.0] {
                let window = egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(
                        (width * scale).round() / scale,
                        (height * scale).round() / scale,
                    ),
                );
                for side in Side::ALL {
                    let layout = pane_layout_at_scale(window, 272.0, side, scale);
                    let (outer, between) = if side == Side::Left {
                        (
                            (
                                layout.sidebar_rect.left() - window.left(),
                                window.right() - layout.page_rect.right(),
                            ),
                            layout.page_rect.left() - layout.sidebar_rect.right(),
                        )
                    } else {
                        (
                            (
                                layout.page_rect.left() - window.left(),
                                window.right() - layout.sidebar_rect.right(),
                            ),
                            layout.sidebar_rect.left() - layout.page_rect.right(),
                        )
                    };
                    let gaps = [
                        outer.0,
                        outer.1,
                        layout.sidebar_rect.top() - window.top(),
                        window.bottom() - layout.sidebar_rect.bottom(),
                        between,
                    ];
                    for gap in gaps {
                        assert!((gap * scale - (8.0 * scale).round()).abs() < 0.001);
                    }
                    assert!((layout.handle_rect.width() - between).abs() < 0.001);
                }
                let classic = classic_page_layout(window, scale);
                assert!(
                    ((classic.left() - window.left()) * scale - (8.0 * scale).round()).abs()
                        < 0.001
                );
                assert!(
                    ((window.right() - classic.right()) * scale - (8.0 * scale).round()).abs()
                        < 0.001
                );
                assert!(
                    ((window.bottom() - classic.bottom()) * scale - (8.0 * scale).round()).abs()
                        < 0.001
                );
            }
        }
    }

    #[test]
    fn early_frame_sleeps_for_remainder() {
        assert_eq!(
            frame_sleep(Duration::from_millis(5), Some(Duration::from_millis(16))),
            Duration::from_millis(11)
        );
    }

    #[test]
    fn late_frame_does_not_sleep() {
        assert_eq!(
            frame_sleep(Duration::from_millis(20), Some(Duration::from_millis(16))),
            Duration::ZERO
        );
    }

    #[test]
    fn idle_gap_does_not_sleep() {
        assert_eq!(
            frame_sleep(Duration::from_secs(10), Some(Duration::from_millis(16))),
            Duration::ZERO
        );
    }

    #[test]
    fn uncapped_frame_does_not_sleep() {
        assert_eq!(frame_sleep(Duration::ZERO, None), Duration::ZERO);
    }
}
