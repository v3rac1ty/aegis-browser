use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use aegis_engine::{Bounds, ContentView, Start, is_http_url};
use aegis_ffi::{BlockRules, Shortcut, SuspendOutcome};
use eframe::egui;

const SIDEBAR_WIDTH: f32 = 260.0;
const HOT_STRIP_WIDTH: f32 = 6.0;
const COMMAND_WIDTH: f32 = 600.0;
const COMMAND_HEIGHT: f32 = 56.0;
const COMMAND_FOCUS_TIMEOUT: Duration = Duration::from_millis(500);
const HOVER_ANIMATION_SECONDS: f32 = 0.16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommandBarState {
    Closed,
    Opening {
        focused_frames: u8,
        started_at: Instant,
    },
    Open,
}

#[derive(Clone, Copy, Debug)]
enum SuspendDisplay {
    Outcome(SuspendOutcome),
    Resumed,
}

pub struct AegisApp {
    start: Option<Start>,
    content_view: Option<ContentView>,
    content_bounds: Option<Bounds>,
    window_scale: Option<f64>,
    command_state: CommandBarState,
    command_text: String,
    compact: bool,
    hover_sidebar_open: bool,
    hover_left_at: Option<Instant>,
    debug_focus: bool,
    last_root_focus: Option<bool>,
    last_command_focus: Option<bool>,
    blocked_count: Rc<Cell<usize>>,
    command_requested: Rc<Cell<bool>>,
    suspend_outcome: Rc<Cell<SuspendDisplay>>,
    error: Option<String>,
}

impl AegisApp {
    pub fn new(start: Start) -> Self {
        Self {
            start: Some(start),
            content_view: None,
            content_bounds: None,
            window_scale: None,
            command_state: CommandBarState::Closed,
            command_text: String::new(),
            compact: false,
            hover_sidebar_open: false,
            hover_left_at: None,
            debug_focus: cfg!(debug_assertions)
                && std::env::var("AEGIS_DEBUG_FOCUS").as_deref() == Ok("1"),
            last_root_focus: None,
            last_command_focus: None,
            blocked_count: Rc::new(Cell::new(0)),
            command_requested: Rc::new(Cell::new(false)),
            suspend_outcome: Rc::new(Cell::new(SuspendDisplay::Resumed)),
            error: None,
        }
    }

    fn open_command_bar(&mut self, reason: &str) {
        if self.command_state != CommandBarState::Closed {
            return;
        }
        self.command_state = CommandBarState::Opening {
            focused_frames: 0,
            started_at: Instant::now(),
        };
        self.command_text.clear();
        if self.debug_focus {
            eprintln!("command bar opened: {reason}");
        }
    }

    fn close_command_bar(&mut self, reason: &str) {
        if self.command_state == CommandBarState::Closed {
            return;
        }
        self.command_state = CommandBarState::Closed;
        if self.debug_focus {
            eprintln!("command bar closed: {reason}");
        }
    }

    fn log_focus(&mut self, viewport: &str, focused: bool) {
        let previous = if viewport == "root" {
            &mut self.last_root_focus
        } else {
            &mut self.last_command_focus
        };
        if *previous != Some(focused) {
            *previous = Some(focused);
            if self.debug_focus {
                eprintln!("{viewport} focus: {focused}");
            }
        }
    }

    fn sidebar_contents(&mut self, ui: &mut egui::Ui) {
        ui.heading("Aegis");
        ui.separator();
        ui.label("Example tab");
        ui.label("Pinned tab");
        ui.label("Another tab");
        ui.add_space(20.0);

        if ui.button("Command bar").clicked() {
            self.open_command_bar("sidebar button");
        }
        if ui.checkbox(&mut self.compact, "Compact mode").changed() {
            self.hover_sidebar_open = false;
            self.hover_left_at = None;
            ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
        }

        #[cfg(target_os = "windows")]
        ui.label(format!("Blocked: {}", self.blocked_count.get()));
        #[cfg(not(target_os = "windows"))]
        ui.label("Blocked: n/a");

        if ui.button("Suspend").clicked()
            && let Some(view) = &self.content_view
        {
            if let Err(error) = aegis_ffi::set_background_memory(view.webview(), true) {
                self.error = Some(error.to_string());
            }
            let outcome = self.suspend_outcome.clone();
            let ctx = ui.ctx().clone();
            if let Err(error) = aegis_ffi::suspend(
                view.webview(),
                Box::new(move |result| {
                    outcome.set(SuspendDisplay::Outcome(result));
                    ctx.request_repaint();
                }),
            ) {
                self.error = Some(error.to_string());
            }
        }
        if ui.button("Resume").clicked()
            && let Some(view) = &self.content_view
        {
            match aegis_ffi::resume(view.webview()) {
                Ok(()) => self.suspend_outcome.set(SuspendDisplay::Resumed),
                Err(error) => self.error = Some(error.to_string()),
            }
            if let Err(error) = aegis_ffi::set_background_memory(view.webview(), false) {
                self.error = Some(error.to_string());
            }
        }
        let status = match self.suspend_outcome.get() {
            SuspendDisplay::Outcome(outcome) => format!("{outcome:?}"),
            SuspendDisplay::Resumed => "Running".to_owned(),
        };
        ui.label(format!("Suspend: {status}"));

        if let Some(error) = &self.error {
            ui.add_space(20.0);
            ui.colored_label(egui::Color32::RED, error);
        }
    }

    fn sync_content_view(&mut self, ctx: &egui::Context, frame: &eframe::Frame, rect: egui::Rect) {
        let Some(window) = frame.winit_window() else {
            return;
        };

        // egui points include UI zoom; wry logical pixels only include OS scaling
        let logical_per_point = f64::from(ctx.pixels_per_point()) / window.scale_factor();
        let bounds = Bounds {
            x: f64::from(rect.min.x) * logical_per_point,
            y: f64::from(rect.min.y) * logical_per_point,
            width: f64::from(rect.width()) * logical_per_point,
            height: f64::from(rect.height()) * logical_per_point,
        };

        if self.content_view.is_none() {
            let Some(start) = self.start.take() else {
                return;
            };
            match ContentView::new(window.as_ref(), bounds, Start::Html(String::new())) {
                Ok(view) => {
                    let blocked_count = self.blocked_count.clone();
                    let repaint = ctx.clone();
                    if let Err(error) = aegis_ffi::install_block_rules(
                        view.webview(),
                        &BlockRules {
                            url_substrings: vec!["aegis-block".to_owned()],
                        },
                        Box::new(move |_| {
                            blocked_count.set(blocked_count.get() + 1);
                            repaint.request_repaint();
                        }),
                    ) {
                        self.error = Some(error.to_string());
                    }
                    let command_requested = self.command_requested.clone();
                    let repaint = ctx.clone();
                    if let Err(error) = aegis_ffi::install_shortcuts(
                        view.webview(),
                        Box::new(move |shortcut| match shortcut {
                            Shortcut::CommandBar => {
                                command_requested.set(true);
                                repaint.request_repaint();
                            }
                            Shortcut::NewTab => eprintln!("new tab shortcut"),
                        }),
                    ) {
                        self.error = Some(error.to_string());
                    }
                    let load_result = match start {
                        Start::Url(url) => view.navigate(&url).map_err(|error| error.to_string()),
                        Start::Html(html) => view
                            .webview()
                            .load_html(&html)
                            .map_err(|error| error.to_string()),
                    };
                    if let Err(error) = load_result {
                        self.error = Some(error);
                    }
                    self.content_view = Some(view);
                    self.content_bounds = Some(bounds);
                    self.window_scale = Some(window.scale_factor());
                }
                Err(error) => self.error = Some(error.to_string()),
            }
            return;
        }

        if self.content_bounds == Some(bounds) && self.window_scale == Some(window.scale_factor()) {
            return;
        }

        if let Some(view) = &self.content_view
            && let Err(error) = view.set_bounds(bounds)
        {
            self.error = Some(error.to_string());
            return;
        }
        self.content_bounds = Some(bounds);
        self.window_scale = Some(window.scale_factor());
    }

    fn show_command_bar(&mut self, ctx: &egui::Context, content: egui::Rect, window: egui::Rect) {
        let width = COMMAND_WIDTH.min(content.width());
        let x = window.min.x + content.center().x - width / 2.0;
        let y = window.min.y + content.min.y + 80.0;
        let builder = egui::ViewportBuilder::default()
            .with_position(egui::pos2(x, y))
            .with_inner_size([width, COMMAND_HEIGHT])
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false);

        let (focused, key_down) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("command_bar"),
            builder,
            |ui, _| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.add_space(7.0);
                    let response = ui.add_sized(
                        [ui.available_width(), 36.0],
                        egui::TextEdit::singleline(&mut self.command_text).hint_text("Enter URL"),
                    );
                    if matches!(self.command_state, CommandBarState::Opening { .. }) {
                        response.request_focus();
                    }

                    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                    let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if escape || enter {
                        self.close_command_bar(if escape { "escape" } else { "enter" });
                        if enter {
                            let url = self.command_text.trim();
                            if is_http_url(url) {
                                if let Some(view) = &self.content_view
                                    && let Err(error) = view.navigate(url)
                                {
                                    self.error = Some(error.to_string());
                                }
                            } else {
                                self.error = Some("Enter an http:// or https:// URL".to_owned());
                            }
                        }
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    }
                });
                ui.input(|input| {
                    (
                        input.viewport().focused == Some(true),
                        input.key_down(egui::Key::L),
                    )
                })
            },
        );
        self.log_focus("overlay", focused);
        if let CommandBarState::Opening {
            focused_frames,
            started_at,
        } = self.command_state
        {
            if !focused && started_at.elapsed() >= COMMAND_FOCUS_TIMEOUT {
                self.close_command_bar("could not take focus");
            } else if focused && !key_down {
                // webview key-up may bypass egui, so give focus a brief time to settle
                if focused_frames >= 1 && started_at.elapsed() >= Duration::from_millis(200) {
                    self.command_state = CommandBarState::Open;
                } else {
                    self.command_state = CommandBarState::Opening {
                        focused_frames: focused_frames.saturating_add(1),
                        started_at,
                    };
                }
            } else {
                self.command_state = CommandBarState::Opening {
                    focused_frames: 0,
                    started_at,
                };
            }
            if !focused && self.command_state != CommandBarState::Closed {
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::from_hash_of("command_bar"),
                    egui::ViewportCommand::Focus,
                );
            }
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        } else if self.command_state == CommandBarState::Open && !focused {
            self.close_command_bar("overlay lost focus");
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    fn show_hover_sidebar(
        &mut self,
        ctx: &egui::Context,
        window: egui::Rect,
        strip_hovered: bool,
        width: f32,
    ) {
        let builder = egui::ViewportBuilder::default()
            .with_position(egui::pos2(window.min.x + HOT_STRIP_WIDTH, window.min.y))
            .with_inner_size([width, window.height()])
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false);

        let sidebar_hovered = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("hover_sidebar"),
            builder,
            |ui, _| {
                let fill = ui.visuals().panel_fill;
                let separator = ui.visuals().widgets.noninteractive.bg_stroke.color;
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(fill))
                    .show(ui, |ui| {
                        let visible = ui.max_rect();
                        // keep the contents full width while the window reveals them
                        let contents = egui::Rect::from_min_size(
                            egui::pos2(visible.right() - SIDEBAR_WIDTH, visible.top()),
                            egui::vec2(SIDEBAR_WIDTH, visible.height()),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(contents), |ui| {
                            self.sidebar_contents(ui);
                        });
                        ui.painter().line_segment(
                            [
                                egui::pos2(visible.right() - 0.5, visible.top()),
                                egui::pos2(visible.right() - 0.5, visible.bottom()),
                            ],
                            egui::Stroke::new(1.0, separator),
                        );
                    });
                ui.input(|input| input.pointer.hover_pos().is_some())
            },
        );

        if sidebar_hovered || strip_hovered {
            self.hover_sidebar_open = true;
            self.hover_left_at = None;
        } else if self.hover_sidebar_open {
            let left_at = *self.hover_left_at.get_or_insert_with(Instant::now);
            let elapsed = left_at.elapsed();
            if elapsed >= Duration::from_millis(250) {
                self.hover_sidebar_open = false;
                self.hover_left_at = None;
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            } else {
                ctx.request_repaint_after_for(
                    Duration::from_millis(250) - elapsed,
                    egui::ViewportId::ROOT,
                );
            }
        }
    }
}

impl eframe::App for AegisApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(target_os = "linux")]
        {
            // webkitgtk needs its main loop pumped alongside winit
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        let root_focused = ctx.input(|input| input.viewport().focused == Some(true));
        self.log_focus("root", root_focused);
        if ctx.input(|input| input.pointer.any_pressed()) {
            self.close_command_bar("main window click");
        }
        if self.command_requested.replace(false) {
            self.open_command_bar("webview shortcut");
        }

        if ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::L)) {
            self.open_command_bar("egui shortcut");
        }

        let mut strip_hovered = false;
        if self.compact {
            egui::Panel::left("hot_strip")
                .resizable(false)
                .exact_size(HOT_STRIP_WIDTH)
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show(ui, |ui| {
                    let rect = ui.max_rect();
                    ui.painter()
                        .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                    strip_hovered = ui
                        .interact(rect, ui.scope_id().with("hover"), egui::Sense::hover())
                        .hovered();
                });
            if strip_hovered {
                self.hover_sidebar_open = true;
                self.hover_left_at = None;
            }
        } else {
            egui::Panel::left("sidebar")
                .resizable(false)
                .exact_size(SIDEBAR_WIDTH)
                .show(ui, |ui| self.sidebar_contents(ui));
        }

        let content = egui::CentralPanel::no_frame()
            .show(ui, |ui| ui.max_rect())
            .inner;
        self.sync_content_view(&ctx, frame, content);

        if let Some(window) = ctx.input(|input| input.viewport().inner_rect) {
            let progress = ctx.animate_bool_with_time_and_easing(
                egui::Id::unique("hover_sidebar_animation"),
                self.compact && self.hover_sidebar_open,
                HOVER_ANIMATION_SECONDS,
                egui::emath::easing::cubic_out,
            );
            let width = (SIDEBAR_WIDTH * progress).min((window.width() - HOT_STRIP_WIDTH).max(0.0));
            if self.compact && width > 0.0 {
                self.show_hover_sidebar(&ctx, window, strip_hovered, width);
            }
            if self.command_state != CommandBarState::Closed {
                self.show_command_bar(&ctx, content, window);
            }
        }
    }
}
