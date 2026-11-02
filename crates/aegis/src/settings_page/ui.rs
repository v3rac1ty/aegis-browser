use eframe::egui::{self, Align, Color32, FontId, Layout, Rect, Sense, Stroke, UiBuilder};

use crate::theme::{self, Theme};

pub const ROW_PAD_Y: f32 = 12.0;
pub const ROW_PAD_X: f32 = 16.0;
const HINT_GAP: f32 = 2.0;
const CONTROL_GAP: f32 = 16.0;
const SECTION_GAP: f32 = 12.0;

/// width and height of a row control
#[derive(Clone, Copy)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

pub const NONE: Size = Size { w: 0.0, h: 0.0 };
pub const SEGMENTED: Size = Size { w: 260.0, h: 34.0 };
pub const SWITCH: Size = Size { w: 44.0, h: 26.0 };
pub const SLIDER: Size = Size { w: 260.0, h: 28.0 };
pub const BUTTON: Size = Size { w: 180.0, h: 28.0 };
pub const INPUT: Size = Size { w: 260.0, h: 30.0 };

/// row height from its label block and control, both sit on the center line
pub fn row_height(block: f32, control: f32) -> f32 {
    block.max(control) + 2.0 * ROW_PAD_Y
}

/// top edge that centers an item of `item` height in a row
pub fn centered_top(top: f32, height: f32, item: f32) -> f32 {
    top + (height - item) * 0.5
}

fn label_wrap(width: f32, control_w: f32) -> f32 {
    let control = if control_w > 0.0 {
        CONTROL_GAP + control_w
    } else {
        0.0
    };
    (width - 2.0 * ROW_PAD_X - control).clamp(120.0, width.max(120.0))
}

/// 12 pt uppercase label, semibold with 0.05 em tracking
pub fn section_label(ui: &mut egui::Ui, theme: Theme, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .font(crate::fonts::semibold(12.0))
            .extra_letter_spacing(0.6)
            .color(theme.palette.secondary),
    );
}

/// card with a label above it, returns the card rect
pub fn section(
    ui: &mut egui::Ui,
    theme: Theme,
    label: &str,
    add: impl FnOnce(&mut egui::Ui),
) -> Rect {
    section_label(ui, theme, label);
    ui.add_space(SECTION_GAP);
    let card = egui::Frame::new()
        .fill(theme.palette.card)
        .stroke(Stroke::new(
            theme::hairline(ui.ctx()),
            theme.palette.card_edge,
        ))
        .corner_radius(14.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let clip = ui.clip_rect();
            let bounds = ui.max_rect();
            ui.set_clip_rect(Rect::from_min_max(
                egui::pos2(clip.left().max(bounds.left()), clip.top()),
                egui::pos2(clip.right().min(bounds.right()), clip.bottom()),
            ));
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
    ui.add_space(26.0);
    card.response.rect
}

pub fn row<R>(
    ui: &mut egui::Ui,
    theme: Theme,
    label: &str,
    size: Size,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    row_hint(ui, theme, label, None, size, control)
}

pub fn row_hint<R>(
    ui: &mut egui::Ui,
    theme: Theme,
    label: &str,
    hint: Option<&str>,
    size: Size,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let width = ui.available_width();
    let wrap = label_wrap(width, size.w);
    let title = ui.painter().layout(
        label.to_owned(),
        FontId::proportional(13.0),
        theme.palette.text,
        wrap,
    );
    let hint = hint.map(|text| {
        ui.painter().layout(
            text.to_owned(),
            FontId::proportional(12.0),
            theme.palette.secondary,
            wrap,
        )
    });
    let title_height = title.size().y;
    let block = title_height + hint.as_ref().map_or(0.0, |g| HINT_GAP + g.size().y);
    let height = row_height(block, size.h);
    // one explicit rect, label block and control share its center line
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let mut y = centered_top(rect.top(), height, block);
    ui.painter().galley(
        egui::pos2(rect.left() + ROW_PAD_X, y),
        title,
        theme.palette.text,
    );
    y += title_height + HINT_GAP;
    if let Some(hint) = hint {
        ui.painter().galley(
            egui::pos2(rect.left() + ROW_PAD_X, y),
            hint,
            theme.palette.secondary,
        );
    }
    let band = Rect::from_center_size(
        rect.center(),
        egui::vec2((width - 2.0 * ROW_PAD_X).max(0.0), size.h),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(band)
            .layout(Layout::right_to_left(Align::Center)),
    );
    control(&mut child)
}

// free-form content inside a card, padded like a row
pub fn block(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(ROW_PAD_X as i8, ROW_PAD_Y as i8))
        .show(ui, add);
}

// hairline between rows, none before the first one
pub fn divider(ui: &mut egui::Ui, theme: Theme, first: &mut bool) {
    if !*first {
        hairline(ui, theme);
    }
    *first = false;
}

pub fn hairline(ui: &mut egui::Ui, theme: Theme) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::hairline(ui.ctx())),
        Sense::hover(),
    );
    ui.painter().rect_filled(
        Rect::from_min_max(rect.min + egui::vec2(ROW_PAD_X, 0.0), rect.max),
        0.0,
        theme.palette.hairline,
    );
}

pub fn segmented(
    ui: &mut egui::Ui,
    label: &str,
    options: &[&str],
    selected: usize,
    theme: Theme,
) -> Option<usize> {
    let width = 260.0_f32.min(ui.available_width().max(0.0));
    let (track, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), Sense::hover());
    ui.painter().rect_filled(track, 10.0, theme.palette.well);
    let segment_width =
        (width - 6.0 - 2.0 * (options.len().saturating_sub(1)) as f32) / options.len() as f32;
    let mut changed = None;
    let thumb_id = ui.scope_id().with((label, "thumb"));
    let mut spring = ui
        .ctx()
        .data_mut(|data| data.get_temp::<crate::motion::Spring>(thumb_id))
        .unwrap_or_else(|| crate::motion::Spring::new(selected as f32));
    spring.target = selected as f32;
    if theme.motion_pref == aegis_core::MotionPref::Reduced {
        spring.value = spring.target;
        spring.velocity = 0.0;
    } else {
        spring.step(ui.input(|input| input.unstable_dt).min(0.1), 0.20);
        if !spring.settled() {
            crate::motion::request_animation_frame(ui.ctx());
        }
    }
    let thumb_x = spring.value;
    ui.ctx().data_mut(|data| data.insert_temp(thumb_id, spring));
    let thumb = Rect::from_min_size(
        egui::pos2(
            track.left() + 3.0 + thumb_x * (segment_width + 2.0),
            track.top() + 3.0,
        ),
        egui::vec2(segment_width, 28.0),
    );
    if !theme.dark {
        ui.painter().add(
            egui::Shadow {
                offset: [0, 1],
                blur: 3,
                spread: 0,
                color: Color32::from_black_alpha(30),
            }
            .as_shape(thumb, 8.0),
        );
    }
    ui.painter().rect_filled(thumb, 8.0, theme.palette.seg_on);
    for (index, option) in options.iter().enumerate() {
        let x = track.left() + 3.0 + index as f32 * (segment_width + 2.0);
        let rect = Rect::from_min_size(
            egui::pos2(x, track.top() + 3.0),
            egui::vec2(segment_width, 28.0),
        );
        let response = ui.interact(rect, ui.scope_id().with((label, index)), Sense::click());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            *option,
            FontId::proportional(13.0),
            theme.palette.text,
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *option));
        if response.clicked() {
            changed = Some(index);
        }
    }
    changed
}

pub fn switch(ui: &mut egui::Ui, value: &mut bool, label: &str, theme: Theme) -> egui::Response {
    crate::widgets::switch(ui, value, label, theme)
}

pub fn slider(
    ui: &mut egui::Ui,
    value: &mut u16,
    range: std::ops::RangeInclusive<u16>,
    step: u16,
    suffix: &str,
    theme: Theme,
) -> bool {
    let before = *value;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(260.0_f32.min(ui.available_width()), 28.0),
        Sense::click_and_drag(),
    );
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let fraction =
            ((pos.x - rect.left() - 8.0) / (rect.width() - 56.0).max(1.0)).clamp(0.0, 1.0);
        let raw = *range.start() as f32 + fraction * (*range.end() - *range.start()) as f32;
        *value = ((raw / step as f32).round() as u16 * step).clamp(*range.start(), *range.end());
    }
    let track = Rect::from_min_size(
        rect.left_center() + egui::vec2(8.0, -2.0),
        egui::vec2((rect.width() - 56.0).max(0.0), 4.0),
    );
    let fraction = (*value - *range.start()) as f32 / (*range.end() - *range.start()).max(1) as f32;
    ui.painter()
        .rect_filled(track, 2.0, crate::widgets::track_color(theme));
    ui.painter().rect_filled(
        Rect::from_min_size(track.min, egui::vec2(track.width() * fraction, 4.0)),
        2.0,
        theme.accent,
    );
    let thumb = egui::pos2(track.left() + track.width() * fraction, track.center().y);
    crate::widgets::thumb(ui.painter(), thumb, 8.0, theme);
    ui.painter().text(
        rect.right_center() - egui::vec2(4.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        format!("{value}{suffix}"),
        FontId::proportional(12.0),
        theme.palette.secondary,
    );
    *value != before
}

pub fn button(ui: &mut egui::Ui, label: &str, theme: Theme, accent: bool) -> egui::Response {
    let width = (label.chars().count() as f32 * 7.0 + 24.0).min(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::click());
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let fill = if accent {
        theme.accent
    } else {
        theme.palette.well
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    if hover > 0.0 {
        ui.painter()
            .rect_filled(rect, 8.0, Color32::WHITE.gamma_multiply(hover * 0.1));
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.0),
        if accent {
            Color32::WHITE
        } else {
            theme.palette.text
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response
}

fn input_frame(
    ui: &mut egui::Ui,
    theme: Theme,
    edit: impl FnOnce(&mut egui::Ui, f32) -> egui::Response,
) -> egui::Response {
    let width = INPUT.w.min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, INPUT.h), Sense::hover());
    ui.painter().rect_filled(rect, 8.0, theme.palette.well);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(8.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    edit(&mut child, rect.width() - 16.0)
}

pub fn text_input(ui: &mut egui::Ui, value: &mut String, theme: Theme) -> egui::Response {
    input_frame(ui, theme, |ui, width| {
        ui.add(
            egui::TextEdit::singleline(value)
                .frame(egui::Frame::NONE)
                .desired_width(width),
        )
    })
}

pub fn password_input(
    ui: &mut egui::Ui,
    value: &mut String,
    hidden: bool,
    theme: Theme,
) -> egui::Response {
    input_frame(ui, theme, |ui, width| {
        ui.add(
            egui::TextEdit::singleline(value)
                .password(hidden)
                .frame(egui::Frame::NONE)
                .desired_width(width),
        )
    })
}

const CHECK_SIZE: f32 = 16.0;
const CHECK_GAP: f32 = 10.0;

pub fn check_icon(ui: &egui::Ui, rect: Rect, active: bool, theme: Theme) {
    let color = if active {
        theme.accent
    } else {
        theme.palette.secondary
    };
    ui.painter().circle_stroke(
        rect.center(),
        rect.width() * 0.5 - 0.75,
        Stroke::new(1.5, color),
    );
    if active {
        crate::icons::check(ui.painter(), rect.shrink(3.0), color);
    } else {
        crate::icons::dash(ui.painter(), rect.shrink(4.0), color);
    }
}

pub fn protection_row(ui: &mut egui::Ui, theme: Theme, title: &str, detail: &str, active: bool) {
    let width = ui.available_width();
    let text_x = ROW_PAD_X + CHECK_SIZE + CHECK_GAP;
    let wrap = (width - text_x - ROW_PAD_X).max(120.0);
    let title = ui.painter().layout(
        title.to_owned(),
        FontId::proportional(13.0),
        theme.palette.text,
        wrap,
    );
    let detail = ui.painter().layout(
        detail.to_owned(),
        FontId::proportional(12.0),
        theme.palette.secondary,
        wrap,
    );
    let title_height = title.size().y;
    let block = title_height + HINT_GAP + detail.size().y;
    let height = row_height(block, CHECK_SIZE);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + ROW_PAD_X + CHECK_SIZE * 0.5, rect.center().y),
        egui::Vec2::splat(CHECK_SIZE),
    );
    check_icon(ui, icon, active, theme);
    let y = centered_top(rect.top(), height, block);
    ui.painter().galley(
        egui::pos2(rect.left() + text_x, y),
        title,
        theme.palette.text,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + text_x, y + title_height + HINT_GAP),
        detail,
        theme.palette.secondary,
    );
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use aegis_core::Settings;

    fn run(add: impl FnOnce(&mut egui::Ui, Theme)) -> egui::FullOutput {
        let ctx = egui::Context::default();
        let theme = theme::resolve(&Settings::default(), false);
        let mut add = Some(add);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 400.0),
                )),
                ..egui::RawInput::default()
            },
            |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if let Some(add) = add.take() {
                    add(ui, theme);
                }
            },
        );
        output.textures_delta.clear();
        output
    }

    fn text_rect(output: &egui::FullOutput, text: &str) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == text => {
                    Some(Rect::from_min_size(shape.pos, shape.galley.size()))
                }
                _ => None,
            })
    }

    #[test]
    fn row_height_is_tallest_item_plus_padding() {
        assert_eq!(row_height(17.0, 34.0), 58.0);
        assert_eq!(row_height(34.0, 26.0), 58.0);
        assert_eq!(row_height(17.0, 0.0), 41.0);
        assert_eq!(centered_top(100.0, 58.0, 34.0), 112.0);
        assert_eq!(centered_top(100.0, 58.0, 17.0), 120.5);
    }

    #[test]
    fn label_block_and_control_share_the_row_center() {
        let hint_text = "Tinted follows the current space's color";
        for hint in [None, Some(hint_text)] {
            let mut row_rect = Rect::NOTHING;
            let mut control_rect = Rect::NOTHING;
            let output = run(|ui, theme| {
                let top = ui.cursor().top();
                let mut on = false;
                control_rect = row_hint(ui, theme, "Floating page", hint, SWITCH, |ui| {
                    switch(ui, &mut on, "Floating page", theme).rect
                });
                row_rect = Rect::from_min_max(
                    egui::pos2(0.0, top),
                    egui::pos2(ui.available_width(), ui.cursor().top()),
                );
            });
            let center = row_rect.center().y;
            assert!((control_rect.center().y - center).abs() < 0.51, "{hint:?}");
            let title = text_rect(&output, "Floating page").unwrap();
            let bottom = match hint {
                Some(hint) => text_rect(&output, hint).unwrap().bottom(),
                None => title.bottom(),
            };
            assert!(
                ((title.top() + bottom) * 0.5 - center).abs() < 0.51,
                "{hint:?}"
            );
            let block = bottom - title.top();
            assert!((row_rect.height() - row_height(block, SWITCH.h)).abs() < 0.51);
        }
    }

    #[test]
    fn segmented_row_is_control_height_plus_padding() {
        let mut height = 0.0;
        let _ = run(|ui, theme| {
            let top = ui.cursor().top();
            row(ui, theme, "Tabs", SEGMENTED, |ui| {
                segmented(ui, "Tabs", &["Sidebar", "Classic tabs"], 0, theme)
            });
            height = ui.cursor().top() - top;
        });
        assert_eq!(height, 58.0);
    }
}
