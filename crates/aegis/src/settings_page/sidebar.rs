use super::*;
use crate::widgets;
use aegis_core::{CloseButton, Setup, SpacesStyle, TabIcons, TabLabel};

#[derive(Clone, Copy)]
pub(super) struct SectionDrag {
    source: usize,
    target: usize,
    grab_offset: f32,
}

fn target_index(pointer_y: f32, list_top: f32, heights: &[f32]) -> usize {
    let mut midpoint = list_top;
    for (index, height) in heights.iter().enumerate() {
        midpoint += height * 0.5;
        if pointer_y < midpoint {
            return index;
        }
        midpoint += height * 0.5;
    }
    heights.len().saturating_sub(1)
}

fn reordered(settings: &Settings, from: usize, to: usize) -> Settings {
    let mut next = settings.clone();
    if from < next.sidebar_sections.len() && to < next.sidebar_sections.len() && from != to {
        let slot = next.sidebar_sections.remove(from);
        next.sidebar_sections.insert(to, slot);
        next.setup = Setup::Custom;
    }
    next
}

fn card_fill(from: Color32, to: Color32, progress: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * progress) as u8;
    Color32::from_rgba_unmultiplied(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
        channel(from.a(), to.a()),
    )
}

fn sidebar_columns(content_width: f32) -> usize {
    if content_width >= 1000.0 { 2 } else { 1 }
}

const SETUP_GAP: f32 = 12.0;
const SETUP_MIN_WIDTH: f32 = 150.0;
const PREVIEW_WIDTH: f32 = 420.0;
const COLUMN_GAP: f32 = 28.0;

const SETUPS: [(Setup, &str); 5] = [
    (Setup::Explorer, "Everything, nicely spaced"),
    (Setup::Focus, "Hides until hovered"),
    (Setup::Power, "Dense, on the right"),
    (Setup::Minimal, "Just tabs"),
    (Setup::Classic, "Tabs across the top"),
];

// 5 across when each gets 150 pt, otherwise 3 and 2
fn setup_columns(width: f32) -> usize {
    if width >= 5.0 * SETUP_MIN_WIDTH + 4.0 * SETUP_GAP {
        5
    } else if width >= 3.0 * 110.0 + 2.0 * SETUP_GAP {
        3
    } else if width >= 2.0 * 110.0 + SETUP_GAP {
        2
    } else {
        1
    }
}

// equal cells in a grid, filled row by row
fn setup_cell(grid: Rect, columns: usize, card_height: f32, index: usize) -> Rect {
    let width = (grid.width() - (columns - 1) as f32 * SETUP_GAP) / columns as f32;
    let (row, column) = (index / columns, index % columns);
    Rect::from_min_size(
        egui::pos2(
            grid.left() + column as f32 * (width + SETUP_GAP),
            grid.top() + row as f32 * (card_height + SETUP_GAP),
        ),
        egui::vec2(width, card_height),
    )
}

/// rects of the cards and section rows from the last frame
#[derive(Clone, Copy)]
pub(super) struct Laid {
    pub cards: [Rect; 3],
    pub rows: [Rect; 5],
}

impl Default for Laid {
    fn default() -> Self {
        Self {
            cards: [Rect::NOTHING; 3],
            rows: [Rect::NOTHING; 5],
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    ui: &mut egui::Ui,
    settings: &Settings,
    theme: Theme,
    motion: &mut crate::motion::Motion,
    drag: &mut Option<SectionDrag>,
    laid: &mut Laid,
    content_width: f32,
    content_height: f32,
) -> Option<Command> {
    let mut next = settings.clone();
    let dt = motion.frame();
    ui.label(
        egui::RichText::new("Start from a setup, then change anything.")
            .size(13.0)
            .color(theme.palette.secondary),
    );
    ui.add_space(12.0);
    let setup = setup_cards(ui, settings, theme, motion, dt);
    ui.add_space(16.0);
    if sidebar_columns(content_width) == 2 {
        let controls_width = (ui.available_width() - PREVIEW_WIDTH - COLUMN_GAP).max(0.0);
        let preview_height = (content_height - 283.0).clamp(240.0, 560.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.vertical(|ui| {
                ui.set_width(controls_width);
                controls(ui, &mut next, theme, motion, drag, laid, dt);
            });
            ui.add_space(COLUMN_GAP);
            ui.vertical(|ui| {
                ui.set_width(PREVIEW_WIDTH);
                preview(ui, &next, theme, preview_height);
            });
        });
    } else {
        preview(ui, &next, theme, 360.0);
        controls(ui, &mut next, theme, motion, drag, laid, dt);
    }
    if let Some(setup) = setup {
        return Some(Command::ApplySetup(setup));
    }
    if drag.is_some() {
        return None;
    }
    if next != *settings {
        next.setup = Setup::Custom;
        return Some(Command::SetSettings(next));
    }
    None
}

// five equal cards in a grid, the picked one is lifted by paint only
fn setup_cards(
    ui: &mut egui::Ui,
    settings: &Settings,
    theme: Theme,
    motion: &mut crate::motion::Motion,
    dt: f32,
) -> Option<Setup> {
    let columns = setup_columns(ui.available_width());
    let width = ui.available_width();
    let cell_width = (width - (columns - 1) as f32 * SETUP_GAP) / columns as f32;
    let wrap = (cell_width - 28.0).max(40.0);
    let blurb_font = egui::FontId::proportional(12.0);
    let blurbs: [_; 5] = std::array::from_fn(|index| {
        ui.painter().layout(
            SETUPS[index].1.to_owned(),
            blurb_font.clone(),
            theme.palette.secondary,
            wrap,
        )
    });
    let blurb_height = blurbs
        .iter()
        .map(|galley| galley.size().y)
        .fold(0.0, f32::max);
    let card_height = 38.0 + blurb_height + 14.0;
    let rows = SETUPS.len().div_ceil(columns);
    let grid_height = rows as f32 * card_height + (rows - 1) as f32 * SETUP_GAP;
    let (grid, _) = ui.allocate_exact_size(egui::vec2(width, grid_height), egui::Sense::hover());
    let mut picked = None;
    for (index, ((kind, _), blurb)) in SETUPS.into_iter().zip(blurbs).enumerate() {
        let base = setup_cell(grid, columns, card_height, index);
        let response = ui.interact(
            base,
            ui.scope_id().with(("setup_card", index)),
            egui::Sense::click(),
        );
        let progress = motion.value(
            ui.ctx(),
            ui.scope_id().with(("setup_selected", index)),
            if settings.setup == kind { 1.0 } else { 0.0 },
            0.24,
            dt,
            settings.motion,
        );
        let hover = motion.value(
            ui.ctx(),
            ui.scope_id().with(("setup_hover", index)),
            if response.hovered() { 1.0 } else { 0.0 },
            0.24,
            dt,
            settings.motion,
        );
        // lift moves the paint, never the layout slot
        let rect = base.translate(egui::vec2(0.0, -6.0 * progress - 2.0 * hover));
        let fill = card_fill(theme.palette.card, theme.palette.seg_on, progress);
        ui.painter().rect_filled(rect, 16.0, fill);
        ui.painter().rect_stroke(
            rect,
            16.0,
            Stroke::new(theme::hairline(ui.ctx()), theme.palette.card_edge),
            StrokeKind::Inside,
        );
        if progress > 0.001 {
            ui.painter().rect_stroke(
                rect,
                16.0,
                Stroke::new(2.0, theme.accent.gamma_multiply(progress)),
                StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.min + egui::vec2(14.0, 14.0),
            egui::Align2::LEFT_TOP,
            kind.label(),
            egui::FontId::proportional(14.0),
            theme.palette.text,
        );
        ui.painter().galley(
            rect.min + egui::vec2(14.0, 38.0),
            blurb,
            theme.palette.secondary,
        );
        if progress > 0.001 {
            let badge = rect.right_top() + egui::vec2(-12.0, 12.0);
            ui.painter().circle_filled(
                badge,
                7.0 * (0.6 + 0.4 * progress),
                theme.accent.gamma_multiply(progress),
            );
            crate::icons::check(
                ui.painter(),
                egui::Rect::from_center_size(badge, egui::vec2(10.0, 10.0)),
                Color32::WHITE.gamma_multiply(progress),
            );
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, kind.label()));
        if response.clicked() {
            picked = Some(kind);
        }
    }
    picked
}

fn controls(
    ui: &mut egui::Ui,
    next: &mut Settings,
    theme: Theme,
    motion: &mut crate::motion::Motion,
    drag: &mut Option<SectionDrag>,
    laid: &mut Laid,
    dt: f32,
) {
    laid.cards[0] = card(ui, theme, "SECTIONS", |ui| {
        let row_height = 38.0;
        let list_top = ui.cursor().top();
        let heights = [row_height; 5];
        ui.spacing_mut().item_spacing.y = 0.0;
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            *drag = None;
        }
        if let Some(state) = drag.as_mut()
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
        {
            state.target = target_index(pointer.y - state.grab_offset, list_top, &heights);
        }
        let mut move_to = None;
        let mut released = false;
        let mut offsets = [0.0; 5];
        let mut lifted = None;
        for (index, offset_slot) in offsets
            .iter_mut()
            .enumerate()
            .take(next.sidebar_sections.len())
        {
            let slot = next.sidebar_sections[index];
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), row_height),
                egui::Sense::hover(),
            );
            laid.rows[index] = rect;
            let drag_rect =
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.right() - 50.0, rect.bottom()));
            let response = ui
                .interact(
                    drag_rect,
                    ui.scope_id().with(("section_row", slot.kind.label())),
                    egui::Sense::click_and_drag(),
                )
                .on_hover_cursor(if drag.is_some_and(|state| state.source == index) {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                });
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Button,
                    true,
                    format!("{}; Alt+Up or Alt+Down to reorder", slot.kind.label()),
                )
            });
            if response.drag_started()
                && drag.is_none()
                && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            {
                *drag = Some(SectionDrag {
                    source: index,
                    target: index,
                    grab_offset: pointer.y - rect.center().y,
                });
            }
            if response.has_focus()
                && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowUp))
                && index > 0
            {
                move_to = Some((index, index - 1));
            }
            if response.has_focus()
                && ui.input(|input| input.modifiers.alt && input.key_pressed(egui::Key::ArrowDown))
                && index + 1 < next.sidebar_sections.len()
            {
                move_to = Some((index, index + 1));
            }
            let desired = if let Some(state) = drag.as_ref() {
                if state.source == index {
                    let pointer = ui.input(|input| input.pointer.interact_pos().map(|pos| pos.y));
                    pointer.map_or(0.0, |y| {
                        (y - state.grab_offset).clamp(
                            list_top + row_height * 0.5,
                            list_top + row_height * (next.sidebar_sections.len() as f32 - 0.5),
                        ) - rect.center().y
                    })
                } else if state.source < state.target
                    && index > state.source
                    && index <= state.target
                {
                    -row_height
                } else if state.source > state.target
                    && index >= state.target
                    && index < state.source
                {
                    row_height
                } else {
                    0.0
                }
            } else {
                0.0
            };
            let dragging = drag.is_some_and(|state| state.source == index);
            let offset = if dragging {
                desired
            } else {
                motion.value(
                    ui.ctx(),
                    ui.scope_id().with(("section_shift", slot.kind.label())),
                    desired,
                    0.2,
                    dt,
                    next.motion,
                )
            };
            let painted = rect.translate(egui::vec2(0.0, offset));
            *offset_slot = offset;
            if dragging {
                lifted = Some((painted.expand(2.0), slot.kind.label(), slot.visible));
            }
            let fill = if dragging {
                theme.palette.seg_on
            } else if response.hovered() {
                theme.palette.hover
            } else {
                Color32::TRANSPARENT
            };
            if !dragging {
                ui.painter().rect_filled(painted, 8.0, fill);
                if index + 1 < next.sidebar_sections.len() {
                    ui.painter().line_segment(
                        [
                            painted.left_bottom() + egui::vec2(16.0, 0.0),
                            painted.right_bottom(),
                        ],
                        Stroke::new(crate::theme::hairline(ui.ctx()), theme.palette.hairline),
                    );
                }
                crate::icons::drag_handle(
                    ui.painter(),
                    egui::Rect::from_min_size(
                        painted.min + egui::vec2(8.0, 10.0),
                        egui::vec2(18.0, 18.0),
                    ),
                    theme.palette.secondary,
                );
                ui.painter().text(
                    painted.left_center() + egui::vec2(34.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    slot.kind.label(),
                    egui::FontId::proportional(13.0),
                    theme.palette.text,
                );
                if response.has_focus() {
                    ui.painter().rect_stroke(
                        painted.shrink(1.0),
                        8.0,
                        Stroke::new(2.0, theme.accent),
                        StrokeKind::Inside,
                    );
                }
            }
            let switch_rect = egui::Rect::from_min_size(
                egui::pos2(painted.right() - 44.0, painted.center().y - 13.0),
                egui::vec2(44.0, 26.0),
            );
            // a child ui does not move the list cursor, a scope would
            let mut switch_ui = ui.new_child(egui::UiBuilder::new().max_rect(switch_rect));
            let mut visible = slot.visible;
            if widgets::switch(&mut switch_ui, &mut visible, slot.kind.label(), theme).clicked() {
                next.sidebar_sections[index].visible = visible;
            }
            if response.drag_stopped() && drag.is_some_and(|state| state.source == index) {
                released = true;
            }
        }
        if let Some((rect, label, visible)) = lifted {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                ui.scope_id().with("lifted_section"),
            ));
            painter.rect_filled(rect.expand(4.0), 10.0, Color32::from_black_alpha(35));
            painter.rect_filled(rect, 8.0, theme.palette.seg_on);
            crate::icons::drag_handle(
                &painter,
                egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 10.0), egui::vec2(18.0, 18.0)),
                theme.palette.secondary,
            );
            painter.text(
                rect.left_center() + egui::vec2(34.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.5),
                theme.palette.text,
            );
            let center = egui::pos2(rect.right() - 21.0, rect.center().y);
            painter.rect_filled(
                egui::Rect::from_center_size(center, egui::vec2(42.0, 24.0)),
                12.0,
                if visible {
                    theme.accent
                } else {
                    theme.palette.well
                },
            );
            painter.circle_filled(
                center + egui::vec2(if visible { 9.0 } else { -9.0 }, 0.0),
                9.0,
                theme.palette.page,
            );
        }
        if let Some(state) = drag.as_ref().filter(|_| !released) {
            let preview = reordered(next, state.source, state.target);
            next.sidebar_sections = preview.sidebar_sections;
        }
        if released && let Some(state) = drag.take() {
            for (index, slot) in next.sidebar_sections.iter().enumerate() {
                let shift = if index == state.source {
                    state.target as i32 - state.source as i32
                } else if state.source < state.target
                    && index > state.source
                    && index <= state.target
                {
                    -1
                } else if state.source > state.target
                    && index >= state.target
                    && index < state.source
                {
                    1
                } else {
                    0
                };
                motion.seed(
                    ui.scope_id().with(("section_shift", slot.kind.label())),
                    offsets[index] - shift as f32 * row_height,
                );
            }
            move_to = Some((state.source, state.target));
        }
        if let Some((from, to)) = move_to {
            *next = reordered(next, from, to);
        }
    });
    laid.cards[1] = card(ui, theme, "PLACEMENT", |ui| {
        if let Some(index) = row(
            ui,
            "Position",
            theme,
            &["Left", "Right"],
            usize::from(next.sidebar_side == Side::Right),
        ) {
            next.sidebar_side = Side::ALL[index];
        }
        super::ui::hairline(ui, theme);
        if let Some(index) = row(
            ui,
            "Density",
            theme,
            &["Compact", "Comfortable", "Spacious"],
            Density::ALL
                .iter()
                .position(|value| *value == next.density)
                .unwrap_or(1),
        ) {
            next.density = Density::ALL[index];
        }
        super::ui::hairline(ui, theme);
        super::ui::row(ui, theme, "Width", super::ui::SLIDER, |ui| {
            super::ui::slider(ui, &mut next.sidebar_width, 220..=360, 4, " pt", theme);
        });
        super::ui::hairline(ui, theme);
        super::ui::row(ui, theme, "Sidebar opacity", super::ui::SLIDER, |ui| {
            let mut opacity = next.sidebar_opacity as u16;
            if super::ui::slider(ui, &mut opacity, 30..=100, 5, "%", theme) {
                next.sidebar_opacity = opacity as u8;
            }
        });
        super::ui::hairline(ui, theme);
        super::ui::row(ui, theme, "Hide until hovered", super::ui::SWITCH, |ui| {
            super::ui::switch(ui, &mut next.compact, "Hide until hovered", theme);
        });
    });
    laid.cards[2] = card(ui, theme, "TABS, PINNED AND SPACES", |ui| {
        if let Some(index) = row(
            ui,
            "Tab label",
            theme,
            &["Title", "Site", "Both"],
            TabLabel::ALL
                .iter()
                .position(|v| *v == next.tab_label)
                .unwrap_or(0),
        ) {
            next.tab_label = TabLabel::ALL[index];
        }
        super::ui::hairline(ui, theme);
        if let Some(index) = row(
            ui,
            "Tab icons",
            theme,
            &["Circles", "Squares", "Off"],
            TabIcons::ALL
                .iter()
                .position(|v| *v == next.tab_icons)
                .unwrap_or(0),
        ) {
            next.tab_icons = TabIcons::ALL[index];
        }
        super::ui::hairline(ui, theme);
        if let Some(index) = row(
            ui,
            "Close button",
            theme,
            &["On hover", "Always", "Never"],
            CloseButton::ALL
                .iter()
                .position(|v| *v == next.close_button)
                .unwrap_or(0),
        ) {
            next.close_button = CloseButton::ALL[index];
        }
        super::ui::hairline(ui, theme);
        if let Some(index) = row(
            ui,
            "Pinned style",
            theme,
            &["Circles", "Tiles"],
            usize::from(next.pinned_style == PinnedStyle::Tiles),
        ) {
            next.pinned_style = PinnedStyle::ALL[index];
        }
        super::ui::hairline(ui, theme);
        if let Some(index) = row(
            ui,
            "Pinned per row",
            theme,
            &["4", "5", "6"],
            next.pinned_per_row.saturating_sub(4) as usize,
        ) {
            next.pinned_per_row = (index + 4) as u8;
        }
        super::ui::hairline(ui, theme);
        if let Some(index) = row(
            ui,
            "Spaces style",
            theme,
            &["Segmented", "Dots", "Icons"],
            SpacesStyle::ALL
                .iter()
                .position(|v| *v == next.spaces_style)
                .unwrap_or(0),
        ) {
            next.spaces_style = SpacesStyle::ALL[index];
        }
    });
}

fn preview(ui: &mut egui::Ui, settings: &Settings, theme: Theme, height: f32) {
    // same label and gap as a card, so both columns start on one line
    super::ui::section_label(ui, theme, "LIVE PREVIEW");
    ui.add_space(12.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    crate::preview::draw_mini(ui.painter(), rect, settings, &theme.into());
    ui.add_space(26.0);
}

#[cfg(test)]
fn sample_browser(settings: &Settings) -> aegis_core::Browser {
    let mut browser = aegis_core::Browser::new();
    let _ = browser.apply(Command::SetSettings(settings.clone()), 0);
    let _ = browser.apply(
        Command::AddWorkspace {
            name: "Work".to_owned(),
            color: aegis_core::GroupColor::Blue,
        },
        0,
    );
    let _ = browser.apply(
        Command::AddWorkspace {
            name: "Read".to_owned(),
            color: aegis_core::GroupColor::Green,
        },
        0,
    );
    let mut ids = [None; 4];
    for (index, (title, url)) in [
        ("Aegis", "https://aegis.example"),
        ("News", "https://news.example"),
        ("Notes", "https://notes.example"),
        ("Search", "https://search.example"),
    ]
    .into_iter()
    .enumerate()
    {
        let _ = browser.apply(
            Command::OpenTab {
                url: url.to_owned(),
                placement: aegis_core::Placement::End,
                activate: true,
                opener: None,
            },
            0,
        );
        if let Some(id) = browser.active_tab() {
            let _ = browser.apply(
                Command::SetTitle {
                    tab: id,
                    title: title.to_owned(),
                },
                0,
            );
            ids[index] = Some(id);
        }
    }
    for id in ids.into_iter().take(2).flatten() {
        let _ = browser.apply(Command::PinTab(id), 0);
    }
    browser
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_targets_row_midpoints_and_clamps() {
        let heights = [20.0, 40.0, 30.0];
        assert_eq!(target_index(-100.0, 10.0, &heights), 0);
        assert_eq!(target_index(19.0, 10.0, &heights), 0);
        assert_eq!(target_index(20.0, 10.0, &heights), 1);
        assert_eq!(target_index(49.0, 10.0, &heights), 1);
        assert_eq!(target_index(50.0, 10.0, &heights), 2);
        assert_eq!(target_index(1000.0, 10.0, &heights), 2);
    }

    #[test]
    fn column_rules_match_content_width() {
        assert_eq!(sidebar_columns(999.0), 1);
        assert_eq!(sidebar_columns(1000.0), 2);
        assert_eq!(setup_columns(798.0), 5);
        assert_eq!(setup_columns(797.0), 3);
        assert_eq!(setup_columns(640.0), 3);
        assert_eq!(setup_columns(316.0), 2);
        // five cards in 3 and 2 keep one width and one height
        let grid = Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(640.0, 400.0));
        let first = setup_cell(grid, 3, 82.0, 0);
        for index in 0..5 {
            let cell = setup_cell(grid, 3, 82.0, index);
            assert!((cell.size() - first.size()).length() < 0.01);
            assert!(grid.contains_rect(cell));
        }
        assert_eq!(setup_cell(grid, 3, 82.0, 3).left(), first.left());
        assert_eq!(setup_cell(grid, 3, 82.0, 3).top(), first.top() + 94.0);
    }

    #[test]
    fn reorder_commits_custom_setup_and_visibility() {
        let mut settings = Settings::default();
        settings.sidebar_sections[0].visible = false;
        let changed = reordered(&settings, 0, 3);
        assert_eq!(changed.setup, Setup::Custom);
        assert_eq!(changed.sidebar_sections[3], settings.sidebar_sections[0]);
        assert_eq!(changed.sidebar_sections[0], settings.sidebar_sections[1]);
        assert_eq!(settings.setup, Setup::Explorer);
    }

    #[test]
    fn sidebar_draws_for_each_setup_at_window_sizes() {
        for (width, height) in [
            (2000.0, 1067.0),
            (1280.0, 800.0),
            (900.0, 600.0),
            (640.0, 480.0),
        ] {
            for setup in [
                Setup::Explorer,
                Setup::Focus,
                Setup::Power,
                Setup::Minimal,
                Setup::Classic,
            ] {
                let ctx = egui::Context::default();
                let mut settings = Settings::default();
                settings.apply_setup(setup);
                let browser = sample_browser(&settings);
                let mut sidebar = crate::sidebar::Sidebar::new();
                for _ in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..egui::RawInput::default()
                        },
                        |ui| {
                            let sidebar_width = f32::from(settings.sidebar_width);
                            ui.allocate_ui_with_layout(
                                egui::vec2(sidebar_width, height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    let _ = sidebar.show(
                                        ui,
                                        &browser,
                                        false,
                                        None,
                                        0,
                                        settings.sidebar_side == Side::Right,
                                    );
                                },
                            );
                        },
                    );
                    output.textures_delta.clear();
                }
            }
        }
    }
}
