use std::time::{Duration, Instant};

use aegis_core::Browser;
use eframe::egui;

use crate::sidebar::{Action, Sidebar};
use crate::theme;

pub struct Compact {
    open: bool,
    left_at: Option<Instant>,
}

impl Compact {
    pub fn new() -> Self {
        Self {
            open: false,
            left_at: None,
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
    ) -> Vec<Action> {
        let progress = ctx.animate_bool_with_time_and_easing(
            egui::Id::unique("hover_sidebar_animation"),
            self.open,
            0.16,
            egui::emath::easing::cubic_out,
        );
        let width = (theme::SIDEBAR_WIDTH * progress)
            .min((window.width() - theme::HOT_STRIP_WIDTH).max(0.0));
        if width <= 0.0 {
            return Vec::new();
        }
        let builder = egui::ViewportBuilder::default()
            .with_position(egui::pos2(
                window.min.x + theme::HOT_STRIP_WIDTH,
                window.min.y,
            ))
            .with_inner_size([width, window.height()])
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
                    .frame(egui::Frame::NONE.fill(theme::palette(ui.ctx()).background))
                    .show(ui, |ui| {
                        let visible = ui.max_rect();
                        let contents = egui::Rect::from_min_size(
                            egui::pos2(
                                visible.right() - theme::SIDEBAR_WIDTH + theme::PADDING,
                                visible.top() + theme::PADDING,
                            ),
                            egui::vec2(
                                theme::SIDEBAR_WIDTH - 2.0 * theme::PADDING,
                                visible.height() - 2.0 * theme::PADDING,
                            ),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(contents), |ui| {
                            actions = sidebar.show(ui, browser, loading, notice);
                        });
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
