use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, StrokeKind};

use crate::sidebar::avatar as letter_for_host;
use crate::theme::{self, Theme};

/// unfilled slider and off-switch track, visible on a card
pub fn track_color(theme: Theme) -> Color32 {
    if theme.dark {
        theme.palette.well
    } else {
        theme.palette.well.gamma_multiply(3.0)
    }
}

/// white knob with a soft shadow and a faint edge
pub fn thumb(painter: &egui::Painter, center: egui::Pos2, radius: f32, theme: Theme) {
    painter.circle_filled(
        center + egui::vec2(0.0, 1.0),
        radius + 1.5,
        Color32::from_black_alpha(32),
    );
    painter.circle_filled(center, radius, Color32::WHITE);
    if !theme.dark {
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0, Color32::from_black_alpha(38)),
        );
    }
}

pub fn non_negative(rect: Rect) -> Rect {
    Rect::from_min_max(
        rect.min,
        egui::pos2(rect.max.x.max(rect.min.x), rect.max.y.max(rect.min.y)),
    )
}

pub fn safe_shrink(rect: Rect, by: f32) -> Rect {
    let rect = non_negative(rect);
    rect.shrink2(egui::vec2(
        by.min(rect.width() * 0.5),
        by.min(rect.height() * 0.5),
    ))
}

pub fn window_drag(ui: &egui::Ui, response: &egui::Response, drag_rect: Rect) {
    if response.drag_started_by(egui::PointerButton::Primary)
        && response
            .interact_pointer_pos()
            .is_some_and(|pos| drag_rect.contains(pos))
    {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if response.double_clicked_by(egui::PointerButton::Primary)
        && response
            .interact_pointer_pos()
            .is_some_and(|pos| drag_rect.contains(pos))
    {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

pub fn letter_avatar(ui: &egui::Ui, rect: Rect, host: &str, theme: Theme, round: bool) {
    let (letter, index) = letter_for_host(host);
    let color = theme::group_color(theme::GROUP_COLORS[index % theme::GROUP_COLORS.len()]);
    ui.painter().rect_filled(
        rect,
        if round {
            rect.width() / 2.0
        } else {
            rect.width() * 0.25
        },
        color,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        FontId::proportional(rect.height() * 0.6),
        if theme.dark {
            theme.palette.text
        } else {
            theme::Palette::dark().text
        },
    );
}

pub fn avatar(ui: &mut egui::Ui, rect: Rect, host: &str, theme: Theme, round: bool) {
    if let Some(texture) = crate::favicons::texture(ui.ctx(), host) {
        ui.put(
            rect,
            egui::Image::new((texture, rect.size())).corner_radius(if round {
                rect.width() / 2.0
            } else {
                4.0
            }),
        );
    } else {
        letter_avatar(ui, rect, host, theme, round);
    }
}

pub fn segment(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    dot: Color32,
    theme: Theme,
) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());
    ui.painter()
        .circle_filled(rect.left_center() + egui::vec2(12.0, 0.0), 3.0, dot);
    ui.painter().text(
        rect.left_center() + egui::vec2(21.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        crate::fonts::id(ui.ctx(), "Caption"),
        theme.palette.text,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            theme.metrics.control_radius,
            Stroke::new(2.0, theme.accent),
            StrokeKind::Inside,
        );
    }
    response
}

pub fn switch(ui: &mut egui::Ui, value: &mut bool, label: &str, theme: Theme) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(44.0, 26.0), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let progress = ui
        .ctx()
        .animate_bool_with_time(response.id.with("switch"), *value, 0.12);
    let mix = |off: u8, on: u8| {
        (f32::from(off) + (f32::from(on) - f32::from(off)) * progress).round() as u8
    };
    let off = track_color(theme);
    let on = theme.accent;
    ui.painter().rect_filled(
        rect,
        13.0,
        Color32::from_rgba_unmultiplied(
            mix(off.r(), on.r()),
            mix(off.g(), on.g()),
            mix(off.b(), on.b()),
            mix(off.a(), on.a()),
        ),
    );
    if progress < 1.0 {
        ui.painter().rect_stroke(
            rect,
            13.0,
            Stroke::new(1.0, theme.palette.hairline),
            StrokeKind::Inside,
        );
    }
    let center = rect.left_center() + egui::vec2(13.0 + 18.0 * progress, 0.0);
    thumb(ui.painter(), center, 11.0, theme);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::CheckBox, true, label));
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            14.0,
            Stroke::new(2.0, theme.accent),
            StrokeKind::Inside,
        );
    }
    response
}

pub fn segmented(
    ui: &mut egui::Ui,
    label: &str,
    options: &[&str],
    selected: usize,
    theme: Theme,
) -> Option<usize> {
    let widths: Vec<f32> = options
        .iter()
        .map(|option| option.chars().count() as f32 * 7.0 + 24.0)
        .collect();
    let total: f32 = widths.iter().sum();
    let (track, _) = ui.allocate_exact_size(egui::vec2(total, 30.0), Sense::hover());
    ui.painter().rect_filled(
        track,
        theme.metrics.control_radius + 2.0,
        theme.palette.well,
    );
    let mut change = None;
    let mut left = track.left();
    for (index, option) in options.iter().enumerate() {
        let rect = Rect::from_min_size(
            egui::pos2(left, track.top()),
            egui::vec2(widths[index], track.height()),
        );
        left = rect.right();
        let response = ui.interact(rect, ui.scope_id().with((label, index)), Sense::click());
        if index == selected {
            ui.painter().rect_filled(
                rect.shrink(2.0),
                theme.metrics.control_radius,
                theme.palette.seg_on,
            );
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *option,
            crate::fonts::id(ui.ctx(), "Caption"),
            theme.palette.text,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{label}: {option}"))
        });
        if response.clicked() {
            change = Some(index);
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                theme.metrics.control_radius,
                Stroke::new(2.0, theme.accent),
                StrokeKind::Inside,
            );
        }
    }
    change
}
