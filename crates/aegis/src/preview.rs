use aegis_core::{
    CloseButton, Density, Layout, PinnedStyle, SectionKind, Settings, Side, SpacesStyle, TabIcons,
    TabLabel,
};
use eframe::egui::{self, Color32, Painter, Rect, Stroke, StrokeKind};

use crate::theme::Theme;

pub struct MiniColors {
    pub page: Color32,
    pub pane: Color32,
    pub pane_scrim: Color32,
    pub ink: Color32,
    pub accent: Color32,
}

impl From<Theme> for MiniColors {
    fn from(theme: Theme) -> Self {
        Self {
            page: theme.palette.page,
            pane: theme.palette.pane,
            pane_scrim: theme.pane_scrim,
            ink: theme.palette.text,
            accent: theme.accent,
        }
    }
}

fn bar(painter: &Painter, rect: Rect, color: Color32) {
    if rect.is_positive() {
        painter.rect_filled(rect, (rect.height() * 0.5).min(4.0), color);
    }
}

fn page(painter: &Painter, rect: Rect, colors: &MiniColors) {
    if !rect.is_positive() {
        return;
    }
    painter.rect_filled(rect, 5.0, colors.page);
    let pad = (rect.width() * 0.08).min(18.0);
    let x = rect.left() + pad;
    let y = rect.top() + (rect.height() * 0.14).min(28.0);
    let width = (rect.width() - pad * 2.0).max(0.0);
    bar(
        painter,
        Rect::from_min_size(egui::pos2(x, y), egui::vec2(width * 0.72, 5.0)),
        colors.accent,
    );
    for (index, fraction) in [0.82, 0.58, 0.68, 0.42].into_iter().enumerate() {
        let y = y + 15.0 + index as f32 * 10.0;
        if y + 3.0 > rect.bottom() {
            break;
        }
        bar(
            painter,
            Rect::from_min_size(egui::pos2(x, y), egui::vec2(width * fraction, 3.0)),
            colors.ink.gamma_multiply(0.25),
        );
    }
}

pub fn draw_mini(painter: &Painter, rect: Rect, config: &Settings, colors: &MiniColors) {
    if rect.width() < 10.0 || rect.height() < 10.0 {
        return;
    }
    let painter = painter.with_clip_rect(rect);
    painter.rect_filled(rect, 7.0, Color32::from_black_alpha(140));
    let bounds = rect.shrink(5.0);
    if config.layout == Layout::Horizontal {
        let strip = Rect::from_min_size(
            bounds.min,
            egui::vec2(bounds.width(), (bounds.height() * 0.19).clamp(15.0, 34.0)),
        );
        painter.rect_filled(strip, 4.0, colors.pane);
        if colors.pane_scrim.a() > 0 {
            painter.rect_filled(strip, 4.0, colors.pane_scrim);
        }
        let tab_width = strip.width() * 0.18;
        for index in 0..3 {
            let tab = Rect::from_min_size(
                strip.min + egui::vec2(5.0 + index as f32 * (tab_width + 3.0), 4.0),
                egui::vec2(tab_width, (strip.height() * 0.42).max(3.0)),
            );
            painter.rect_filled(
                tab,
                3.0,
                if index == 0 {
                    colors.accent.gamma_multiply(0.5)
                } else {
                    colors.ink.gamma_multiply(0.16)
                },
            );
        }
        let capsule = Rect::from_center_size(
            egui::pos2(strip.center().x, strip.bottom() - strip.height() * 0.23),
            egui::vec2(strip.width() * 0.47, strip.height() * 0.24),
        );
        painter.rect_filled(capsule, 6.0, colors.ink.gamma_multiply(0.24));
        page(
            &painter,
            Rect::from_min_max(egui::pos2(bounds.left(), strip.bottom() + 4.0), bounds.max),
            colors,
        );
        return;
    }
    let side_width = (bounds.width() * f32::from(config.sidebar_width) / 677.0)
        .clamp(12.0, bounds.width() * 0.65);
    let side = if config.sidebar_side == Side::Right {
        Rect::from_min_max(
            egui::pos2(bounds.right() - side_width, bounds.top()),
            bounds.max,
        )
    } else {
        Rect::from_min_size(bounds.min, egui::vec2(side_width, bounds.height()))
    };
    let pane = if config.sidebar_side == Side::Right {
        Rect::from_min_max(bounds.min, egui::pos2(side.left() - 4.0, bounds.bottom()))
    } else {
        Rect::from_min_max(egui::pos2(side.right() + 4.0, bounds.top()), bounds.max)
    };
    page(&painter, pane, colors);
    if config.compact {
        painter.rect_filled(
            Rect::from_min_size(side.min, egui::vec2(3.0, side.height())),
            1.0,
            colors.accent,
        );
        return;
    }
    painter.rect_filled(side, 5.0, colors.pane);
    if colors.pane_scrim.a() > 0 {
        painter.rect_filled(side, 5.0, colors.pane_scrim);
    }
    let pad = (side.width() * 0.07).max(2.0);
    let inner = side.shrink2(egui::vec2(pad, 5.0));
    bar(
        &painter,
        Rect::from_min_size(inner.min, egui::vec2(inner.width() * 0.37, 3.0)),
        colors.ink.gamma_multiply(0.35),
    );
    let sections = config.sidebar_sections.iter().filter(|slot| slot.visible);
    let count = sections.clone().count();
    let fixed: f32 = sections
        .clone()
        .map(|slot| match slot.kind {
            SectionKind::Spaces => 17.0,
            SectionKind::Pinned => 28.0,
            SectionKind::NewTab => 12.0,
            SectionKind::Capsule => 16.0,
            SectionKind::Tabs => 0.0,
        })
        .sum();
    let tabs_height = (inner.height() - fixed - count as f32 * 5.0 - 12.0).max(20.0);
    let scale = (inner.height() - 12.0) / (fixed + tabs_height + count as f32 * 5.0);
    let mut y = inner.top() + 12.0;
    for slot in sections {
        let height = (match slot.kind {
            SectionKind::Spaces => 17.0,
            SectionKind::Pinned => 28.0,
            SectionKind::NewTab => 12.0,
            SectionKind::Capsule => 16.0,
            SectionKind::Tabs => tabs_height,
        }) * scale;
        let block = Rect::from_min_size(
            egui::pos2(inner.left(), y),
            egui::vec2(inner.width(), height),
        );
        match slot.kind {
            SectionKind::Spaces => {
                if config.spaces_style == SpacesStyle::Segmented {
                    painter.rect_filled(block.shrink(1.0), 3.0, colors.ink.gamma_multiply(0.12));
                }
                for index in 0..3 {
                    let center = egui::pos2(
                        block.left() + block.width() * (index as f32 + 0.5) / 3.0,
                        block.center().y,
                    );
                    if config.spaces_style == SpacesStyle::Icons {
                        painter.rect_filled(
                            Rect::from_center_size(center, egui::vec2(8.0, 8.0)),
                            2.0,
                            colors
                                .accent
                                .gamma_multiply(if index == 0 { 0.8 } else { 0.3 }),
                        );
                    } else {
                        painter.circle_filled(
                            center,
                            if config.spaces_style == SpacesStyle::Dots {
                                3.0
                            } else {
                                2.0
                            },
                            colors
                                .accent
                                .gamma_multiply(if index == 0 { 0.9 } else { 0.35 }),
                        );
                    }
                }
            }
            SectionKind::Pinned => {
                for index in 0..usize::from(config.pinned_per_row) {
                    let width = block.width() / f32::from(config.pinned_per_row);
                    let center = egui::pos2(
                        block.left() + width * (index as f32 + 0.5),
                        block.center().y,
                    );
                    let color = colors.accent.gamma_multiply(0.55 + index as f32 * 0.05);
                    if config.pinned_style == PinnedStyle::Tiles {
                        painter.rect_filled(
                            Rect::from_center_size(center, egui::vec2(width.min(14.0) - 2.0, 12.0)),
                            3.0,
                            color,
                        );
                    } else {
                        painter.circle_filled(center, (width * 0.34).min(6.0), color);
                    }
                }
            }
            SectionKind::Tabs => {
                let row_height = match config.density {
                    Density::Compact => 10.0,
                    Density::Comfortable => 12.0,
                    Density::Spacious => 15.0,
                } * scale;
                for index in 0..5 {
                    let row_y = block.top() + index as f32 * row_height;
                    if row_y + row_height > block.bottom() {
                        break;
                    }
                    if index == 0 {
                        painter.rect_filled(
                            Rect::from_min_size(
                                egui::pos2(block.left(), row_y),
                                egui::vec2(block.width(), row_height - 1.0),
                            ),
                            3.0,
                            colors.accent.gamma_multiply(0.18),
                        );
                    }
                    let icon_x = block.left() + 5.0;
                    if config.tab_icons == TabIcons::Circles {
                        painter.circle_filled(
                            egui::pos2(icon_x, row_y + row_height * 0.5),
                            2.8,
                            colors.accent,
                        );
                    }
                    if config.tab_icons == TabIcons::Squares {
                        painter.rect_filled(
                            Rect::from_center_size(
                                egui::pos2(icon_x, row_y + row_height * 0.5),
                                egui::vec2(5.0, 5.0),
                            ),
                            1.0,
                            colors.accent,
                        );
                    }
                    let label_x = if config.tab_icons == TabIcons::Off {
                        icon_x
                    } else {
                        icon_x + 7.0
                    };
                    let fraction = match config.tab_label {
                        TabLabel::Title => 0.55,
                        TabLabel::Site => 0.4,
                        TabLabel::Both => 0.75,
                    };
                    bar(
                        &painter,
                        Rect::from_min_size(
                            egui::pos2(label_x, row_y + row_height * 0.38),
                            egui::vec2((block.right() - label_x) * fraction, 2.0),
                        ),
                        colors.ink.gamma_multiply(0.55),
                    );
                    if config.close_button == CloseButton::Always {
                        painter.line_segment(
                            [
                                egui::pos2(block.right() - 5.0, row_y + 3.0),
                                egui::pos2(block.right() - 3.0, row_y + 5.0),
                            ],
                            Stroke::new(1.0, colors.ink),
                        );
                    }
                }
            }
            SectionKind::NewTab => bar(
                &painter,
                block.shrink2(egui::vec2(4.0, height * 0.35)),
                colors.ink.gamma_multiply(0.25),
            ),
            SectionKind::Capsule => {
                painter.rect_filled(
                    block.shrink(1.0),
                    height * 0.4,
                    colors.ink.gamma_multiply(0.16),
                );
                bar(
                    &painter,
                    Rect::from_min_size(
                        block.min + egui::vec2(7.0, height * 0.45),
                        egui::vec2(block.width() * 0.48, 2.0),
                    ),
                    colors.ink.gamma_multiply(0.5),
                );
            }
        }
        y += height + 5.0 * scale;
    }
    painter.rect_stroke(
        side,
        5.0,
        Stroke::new(1.0, colors.ink.gamma_multiply(0.12)),
        StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_setups_draw_shapes_at_two_sizes() {
        for setup in [
            aegis_core::Setup::Explorer,
            aegis_core::Setup::Focus,
            aegis_core::Setup::Power,
            aegis_core::Setup::Minimal,
            aegis_core::Setup::Classic,
        ] {
            for size in [egui::vec2(100.0, 80.0), egui::vec2(420.0, 560.0)] {
                let ctx = egui::Context::default();
                let mut settings = Settings::default();
                settings.apply_setup(setup);
                let theme = crate::theme::resolve(&settings, true);
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let rect = Rect::from_min_size(egui::Pos2::ZERO, size);
                    draw_mini(ui.painter(), rect, &settings, &theme.into());
                });
                assert!(
                    output
                        .shapes
                        .iter()
                        .any(|shape| !matches!(shape.shape, egui::Shape::Noop))
                );
                output.textures_delta.clear();
            }
        }
    }
}
