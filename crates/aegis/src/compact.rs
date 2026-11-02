use std::time::{Duration, Instant};

use aegis_core::Browser;
use eframe::egui;

use crate::motion::Motion;
use crate::sidebar::{Action, Sidebar};
use crate::theme;

pub struct Compact {
    open: bool,
    left_at: Option<Instant>,
    motion: Motion,
}

impl Compact {
    pub fn new() -> Self {
        let mut motion = Motion::new();
        motion.seed(egui::Id::unique("hover_sidebar_animation"), 0.0);
        Self {
            open: false,
            left_at: None,
            motion,
        }
    }

    pub fn reset(&mut self) {
        self.open = false;
        self.left_at = None;
    }

    pub fn hot_strip(&mut self, ui: &mut egui::Ui) -> bool {
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
        let hovered = ui
            .interact(rect, ui.scope_id().with("hover"), egui::Sense::hover())
            .hovered();
        if hovered {
            self.open = true;
            self.left_at = None;
        }
        hovered
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        window: egui::Rect,
        strip_hovered: bool,
        sidebar: &mut Sidebar,
        browser: &Browser,
        loading: bool,
        notice: Option<&str>,
        blocked: u32,
        right: bool,
        sidebar_width: f32,
    ) -> Vec<Action> {
        let dt = self.motion.frame();
        let progress = self.motion.value(
            ctx,
            egui::Id::unique("hover_sidebar_animation"),
            if self.open { 1.0 } else { 0.0 },
            0.34,
            dt,
            browser.settings().motion,
        );
        ctx.tessellation_options_mut(|options| {
            let rounded = progress <= 0.0 || progress >= 1.0;
            options.round_text_to_pixels = rounded;
            options.round_rects_to_pixels = rounded;
            options.round_line_segments_to_pixels = rounded;
        });
        let pane_width = (sidebar_width - 16.0)
            .min(window.width() - theme::HOT_STRIP_WIDTH - 16.0)
            .max(0.0);
        let pane_height = (window.height() - 16.0).max(0.0);
        if progress <= 0.0 || pane_width <= 0.0 || pane_height <= 0.0 {
            return Vec::new();
        }
        let visible_width = (pane_width * progress).max(1.0);
        let builder = egui::ViewportBuilder::default()
            .with_icon(crate::logo::icon())
            .with_position(egui::pos2(
                if right {
                    window.max.x - theme::HOT_STRIP_WIDTH - 8.0 - visible_width
                } else {
                    window.min.x + theme::HOT_STRIP_WIDTH + 8.0
                },
                window.min.y + 8.0,
            ))
            .with_inner_size([visible_width, pane_height])
            .with_transparent(true)
            .with_has_shadow(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false);
        let (hovered, actions) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("hover_sidebar"),
            builder,
            |ui, _| {
                let mut actions = Vec::new();
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let visible = ui.max_rect();
                        let contents = egui::Rect::from_min_size(
                            egui::pos2(
                                if right {
                                    visible.left()
                                } else {
                                    visible.right() - pane_width
                                },
                                visible.top(),
                            ),
                            egui::vec2(pane_width, visible.height()),
                        );
                        let contents = crate::widgets::non_negative(contents);
                        if contents.width() >= 1.0 && contents.height() >= 1.0 {
                            ui.scope_builder(egui::UiBuilder::new().max_rect(contents), |ui| {
                                actions =
                                    sidebar.show(ui, browser, loading, notice, blocked, right);
                            });
                        }
                    });
                (
                    ui.input(|input| input.pointer.hover_pos().is_some()),
                    actions,
                )
            },
        );
        if hovered || strip_hovered {
            self.open = true;
            self.left_at = None;
        } else if self.open {
            let left_at = *self.left_at.get_or_insert_with(Instant::now);
            let elapsed = left_at.elapsed();
            if elapsed >= Duration::from_millis(250) {
                self.reset();
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            } else {
                ctx.request_repaint_after_for(
                    Duration::from_millis(250) - elapsed,
                    egui::ViewportId::ROOT,
                );
            }
        }
        actions
    }
}
