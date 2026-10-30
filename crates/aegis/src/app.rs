use std::path::PathBuf;
use std::time::{Duration, Instant};

use aegis_core::{Browser, Command, Effect, Placement, TabId, session};
use aegis_engine::{Engine, Start};
use aegis_ffi::Shortcut;
use eframe::egui;

use crate::command_bar::{self, BarAction, CommandBar, Mode, SuggestionAction};
use crate::compact::Compact;
use crate::persist::SessionSaver;
use crate::sidebar::{Action, Sidebar};
use crate::theme;
use crate::views::{ExecuteResult, Views};

pub struct AegisApp {
    browser: Browser,
    // drop views before the engine so the shared web context stays live
    views: Views,
    engine: Engine,
    sidebar: Sidebar,
    command_bar: CommandBar,
    compact: Compact,
    saver: SessionSaver,
    saving_disabled: bool,
    started: Instant,
    initial: Vec<Command>,
    deferred_views: Vec<TabId>,
    test_page_pending: bool,
    notice: Option<String>,
    #[cfg(debug_assertions)]
    frame_count: Option<u64>,
}

impl AegisApp {
    pub fn new(start: Option<Start>, data_dir: PathBuf, ctx: egui::Context) -> Self {
        ctx.set_theme(egui::ThemePreference::System);
        let directory_error = std::fs::create_dir_all(&data_dir)
            .err()
            .map(|error| format!("Data directory: {error}"));
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
        let notice = notice.or(directory_error);
        let engine = Engine::new(data_dir, Box::new(move || ctx.request_repaint()));
        Self {
            browser,
            views: Views::new(),
            engine,
            sidebar: Sidebar::new(),
            command_bar: CommandBar::new(),
            compact: Compact::new(),
            saver: SessionSaver::new(session_path),
            saving_disabled,
            started: Instant::now(),
            initial,
            deferred_views: Vec::new(),
            test_page_pending,
            notice,
            #[cfg(debug_assertions)]
            frame_count: (std::env::var("AEGIS_DEBUG_FRAMES").as_deref() == Ok("1")).then_some(0),
        }
    }

    fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn apply(&mut self, command: Command, frame: &eframe::Frame, ctx: &egui::Context) {
        let is_test_open = self.test_page_pending
            && matches!(&command, Command::OpenTab { url, .. } if url == "about:blank");
        let effects = match self.browser.apply(command, self.now_ms()) {
            Ok(effects) => effects,
            Err(error) => {
                self.notice = Some(error.to_string());
                return;
            }
        };
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
            match self.views.execute(
                effect,
                &mut self.engine,
                frame,
                ctx,
                self.browser.active_tab(),
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
        let keys = ctx.input(|input| {
            let command = input.modifiers.command;
            let ctrl = input.modifiers.ctrl;
            let shift = input.modifiers.shift;
            let alt = input.modifiers.alt;
            let mut keys = Vec::new();
            if command && input.key_pressed(egui::Key::T) {
                keys.push(Shortcut::NewTab);
            }
            if command && input.key_pressed(egui::Key::L) {
                keys.push(Shortcut::CommandBar);
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
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(debug_assertions)]
        self.count_frame();
        let deferred = std::mem::take(&mut self.deferred_views);
        #[cfg(target_os = "linux")]
        {
            // webkitgtk needs its main loop pumped alongside winit
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        self.keyboard(&ctx, frame);
        let (commands, notices) =
            self.views
                .drain_events(&self.engine, &self.browser, self.now_ms());
        for notice in notices {
            self.notice = Some(notice);
        }
        for command in commands {
            self.apply(command, frame, &ctx);
        }
        let mut actions = Vec::new();
        let mut strip_hovered = false;
        if self.browser.settings().compact {
            egui::Panel::left("hot_strip")
                .resizable(false)
                .exact_size(theme::HOT_STRIP_WIDTH)
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show(ui, |ui| {
                    strip_hovered = self.compact.hot_strip(ui);
                });
        } else {
            egui::Panel::left("sidebar")
                .resizable(false)
                .exact_size(theme::SIDEBAR_WIDTH)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::palette(&ctx).background)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show(ui, |ui| {
                    actions.extend(self.sidebar.show(
                        ui,
                        &self.browser,
                        self.views.active_loading(&self.browser),
                        self.notice.as_deref(),
                    ));
                });
        }
        let content = egui::CentralPanel::no_frame()
            .show(ui, |ui| ui.max_rect())
            .inner;
        if let Err(error) = self.views.set_bounds(&ctx, frame, content) {
            self.notice = Some(error);
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
            if self.browser.settings().compact {
                actions.extend(self.compact.show(
                    &ctx,
                    window,
                    strip_hovered,
                    &mut self.sidebar,
                    &self.browser,
                    self.views.active_loading(&self.browser),
                    self.notice.as_deref(),
                ));
            }
            if let Some(action) = self.command_bar.show(&ctx, content, window, &self.browser) {
                self.run_bar_action(action, frame, &ctx);
            }
        }
        for action in actions {
            self.action(action, frame, &ctx);
        }
        self.save_due();
        if let Some(delay) = self.saver.time_until_due(self.now_ms()) {
            ctx.request_repaint_after(delay);
        }
        if !self.deferred_views.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.saving_disabled {
            return;
        }
        if let Err(error) = self.saver.save(&self.browser) {
            eprintln!("session save: {error}");
        }
    }
}
