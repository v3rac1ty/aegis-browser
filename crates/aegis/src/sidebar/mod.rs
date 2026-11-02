mod capsule;
mod spaces;
mod tabs;

pub(crate) use self::tabs::display_host;
use aegis_core::{
    Browser, Command, Group, GroupColor, GroupId, Item, Location, SectionKind, TabId,
};
use eframe::egui::{self, Color32, Sense, Stroke, StrokeKind};

use crate::command_bar::Mode;
use crate::motion::Motion;
use crate::theme::{self, Palette};

pub enum Action {
    Command(Command),
    OpenBar(Mode),
    Back,
    Forward,
    Reload,
    Stop,
    DismissNotice,
    SetCompact(bool),
    OpenSettings,
    OpenPasswords,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Pinned(usize),
    Item(usize),
    Group(GroupId, usize),
}

pub fn drop_location(target: DropTarget, lower_half: bool) -> Location {
    match target {
        DropTarget::Pinned(index) => Location::Pinned(index + usize::from(lower_half)),
        DropTarget::Item(index) => Location::Items(index + usize::from(lower_half)),
        DropTarget::Group(id, index) => Location::Group(id, index + usize::from(lower_half)),
    }
}

pub fn location_after_removal(browser: &Browser, tab: TabId, to: Location) -> Location {
    let Some(workspace) = browser.workspace(browser.active_workspace()) else {
        return to;
    };
    match to {
        Location::Pinned(mut index) => {
            if let Some(source) = workspace.pinned().iter().position(|id| *id == tab)
                && source < index
            {
                index -= 1;
            }
            Location::Pinned(index)
        }
        Location::Items(mut index) => {
            for (source, item) in workspace.items().iter().enumerate() {
                let removed = match item {
                    Item::Tab(id) => *id == tab,
                    Item::Group(group) => group.tabs().len() == 1 && group.tabs().contains(&tab),
                };
                if removed && source < index {
                    index -= 1;
                    break;
                }
            }
            Location::Items(index)
        }
        Location::Group(group_id, mut index) => {
            for item in workspace.items() {
                if let Item::Group(group) = item
                    && group.id() == group_id
                    && let Some(source) = group.tabs().iter().position(|id| *id == tab)
                    && source < index
                {
                    index -= 1;
                }
            }
            Location::Group(group_id, index)
        }
    }
}

pub fn avatar(host: &str) -> (char, usize) {
    let letter = host
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .unwrap_or('?')
        .to_ascii_uppercase();
    let mut hash = 2166136261_u32;
    for byte in host.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
    }
    (letter, (hash as usize) % 8)
}

// inner padding of the pane, matches the mockup aside
pub(crate) const PANE_PADDING: f32 = 10.0;
const NOTICE_PAD: f32 = 10.0;
const NOTICE_GAP: f32 = 8.0;

fn notice_galley(
    ui: &egui::Ui,
    text: &str,
    width: f32,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    // text, then the close button, inside the card padding
    let wrap = (width - NOTICE_PAD * 2.0 - 24.0).max(20.0);
    ui.painter().layout(
        text.to_owned(),
        crate::fonts::id(ui.ctx(), "Caption"),
        color,
        wrap,
    )
}

fn notice_height(text_height: f32) -> f32 {
    (text_height + 16.0).max(30.0)
}

fn tab_list_height(available: f32, trailing: f32) -> f32 {
    (available - trailing).max(40.0)
}

fn new_tab_fits_after_tabs(
    content_height: f32,
    available: f32,
    trailing: f32,
    row_height: f32,
) -> bool {
    content_height + row_height <= tab_list_height(available, trailing - row_height)
}

fn section_gap_after(current: SectionKind, next: Option<SectionKind>) -> f32 {
    // the last section ends at the pane padding, no gap after it
    if next.is_none() || (current == SectionKind::Tabs && next == Some(SectionKind::NewTab)) {
        0.0
    } else {
        8.0
    }
}

fn pinned_section_height(settings: &aegis_core::Settings, width: f32, count: usize) -> f32 {
    let per_row = settings.pinned_per_row as usize;
    let limit = if settings.pinned_style == aegis_core::PinnedStyle::Tiles {
        44.0
    } else {
        40.0
    };
    let size = ((width - 8.0 * (per_row as f32 - 1.0)) / per_row as f32).min(limit);
    if size < 17.0 {
        return 8.0;
    }
    count.div_ceil(per_row) as f32 * (size + 4.0) + 16.0
}

#[derive(Clone, Copy)]
enum Menu {
    Tab(TabId, egui::Pos2),
    Group(GroupId, egui::Pos2),
}

pub struct Sidebar {
    menu: Option<Menu>,
    rename: Option<(GroupId, String)>,
    theme: theme::Theme,
    privacy_open: bool,
    motion: Motion,
    frame_dt: f32,
    right: bool,
}

impl Sidebar {
    pub fn new() -> Self {
        Self {
            menu: None,
            rename: None,
            theme: theme::resolve(&aegis_core::Settings::default(), false),
            privacy_open: false,
            motion: Motion::new(),
            frame_dt: 0.0,
            right: false,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        loading: bool,
        notice: Option<&str>,
        blocked: u32,
        right: bool,
    ) -> Vec<Action> {
        self.right = right;
        let mut actions = Vec::new();
        self.theme = theme::resolve(
            browser.settings(),
            ui.ctx().system_theme() == Some(egui::Theme::Dark),
        );
        self.frame_dt = self.motion.frame();
        let palette = self.theme.palette;
        let full = ui.max_rect();
        ui.painter()
            .rect_filled(full, self.theme.metrics.pane_radius, palette.pane);
        if self.theme.pane_scrim.a() > 0 {
            ui.painter()
                .rect_filled(full, self.theme.metrics.pane_radius, self.theme.pane_scrim);
        }
        ui.painter().rect_stroke(
            full,
            self.theme.metrics.pane_radius,
            Stroke::new(theme::hairline(ui.ctx()), palette.pane_edge),
            StrokeKind::Inside,
        );
        ui.painter().line_segment(
            [
                full.left_top() + egui::vec2(12.0, 1.0),
                full.right_top() - egui::vec2(12.0, -1.0),
            ],
            Stroke::new(theme::hairline(ui.ctx()), palette.pane_highlight),
        );
        let inner = crate::widgets::safe_shrink(full, PANE_PADDING);
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            self.content(ui, browser, loading, notice, blocked, &mut actions);
        });
        actions
    }

    fn content(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        loading: bool,
        notice: Option<&str>,
        blocked: u32,
        actions: &mut Vec<Action>,
    ) {
        let palette = self.theme.palette;
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        self.title_row(ui, browser.settings().compact, actions);
        ui.add_space(8.0);
        let Some(workspace) = browser.workspace(browser.active_workspace()) else {
            return;
        };
        let mut new_tab_inside = false;
        let mut notice_left = notice;
        for (index, slot) in browser.settings().sidebar_sections.iter().enumerate() {
            if !slot.visible {
                continue;
            }
            match slot.kind {
                SectionKind::Spaces => self.spaces(ui, browser, actions),
                SectionKind::Pinned => {
                    self.pinned(ui, browser, workspace.pinned(), palette, actions)
                }
                SectionKind::Tabs => {
                    new_tab_inside =
                        self.tabs_section(ui, browser, workspace, palette, notice, actions)
                }
                SectionKind::NewTab if !new_tab_inside => {
                    self.new_tab_row(ui, workspace.items().len(), palette, actions)
                }
                SectionKind::NewTab => {}
                SectionKind::Capsule => {
                    if let Some(text) = notice_left.take() {
                        self.notice_card(ui, text, actions);
                    }
                    self.capsule(ui, browser, loading, blocked, actions);
                }
            }
            let next = browser.settings().sidebar_sections[index + 1..]
                .iter()
                .find(|slot| slot.visible)
                .map(|slot| slot.kind);
            ui.add_space(section_gap_after(slot.kind, next));
        }
        if let Some(text) = notice_left {
            self.notice_card(ui, text, actions);
        }
        self.menu(ui, browser, palette, actions);
    }

    // small dismissable card above the capsule, wraps long text
    fn notice_card(&mut self, ui: &mut egui::Ui, text: &str, actions: &mut Vec<Action>) {
        let palette = self.theme.palette;
        let width = ui.available_width();
        let galley = notice_galley(ui, text, width, palette.secondary);
        let height = notice_height(galley.size().y);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
        ui.painter().rect_filled(rect, 10.0, palette.well);
        ui.painter().galley(
            egui::pos2(
                rect.left() + NOTICE_PAD,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            palette.secondary,
        );
        let close_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - NOTICE_PAD - 9.0, rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        let close = ui.interact(
            close_rect,
            ui.scope_id().with("notice_close"),
            Sense::click(),
        );
        crate::icons::button_background(ui, &close, close_rect, palette.hover, palette.seg_on, 6.0);
        crate::icons::close(ui.painter(), close_rect.shrink(4.0), palette.secondary);
        close.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Dismiss notice"));
        crate::icons::focus_ring(ui, &close, close_rect, self.theme.accent, 6.0);
        if close.clicked() {
            actions.push(Action::DismissNotice);
        }
        ui.add_space(NOTICE_GAP);
    }

    fn tabs_section(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        workspace: &aegis_core::Workspace,
        palette: Palette,
        notice: Option<&str>,
        actions: &mut Vec<Action>,
    ) -> bool {
        let notice_reserved = notice.map_or(0.0, |text| {
            let galley = notice_galley(ui, text, ui.available_width(), palette.secondary);
            notice_height(galley.size().y) + NOTICE_GAP
        });
        let capsule_visible = browser
            .settings()
            .sidebar_sections
            .iter()
            .any(|slot| slot.kind == SectionKind::Capsule && slot.visible);
        let mut reserved = if capsule_visible {
            0.0
        } else {
            notice_reserved
        };
        let mut after_tabs = false;
        let mut has_trailing = false;
        for slot in &browser.settings().sidebar_sections {
            if slot.kind == SectionKind::Tabs {
                after_tabs = true;
                continue;
            }
            if !after_tabs || !slot.visible {
                continue;
            }
            reserved += match slot.kind {
                SectionKind::Spaces => {
                    if browser.settings().spaces_style == aegis_core::SpacesStyle::Segmented {
                        40.0
                    } else {
                        36.0
                    }
                }
                SectionKind::Pinned => pinned_section_height(
                    browser.settings(),
                    ui.available_width(),
                    workspace.pinned().len(),
                ),
                SectionKind::NewTab => self.theme.metrics.row_height + 8.0,
                SectionKind::Capsule => 46.0 + notice_reserved,
                SectionKind::Tabs => 0.0,
            };
            has_trailing = true;
        }
        let next = browser
            .settings()
            .sidebar_sections
            .iter()
            .skip_while(|slot| slot.kind != SectionKind::Tabs)
            .skip(1)
            .find(|slot| slot.visible)
            .map(|slot| slot.kind);
        reserved += section_gap_after(SectionKind::Tabs, next);
        if has_trailing {
            // the last section has no gap after it
            reserved -= 8.0;
        }
        let mut new_tab_inside = false;
        if next == Some(SectionKind::NewTab) {
            let content_height: f32 = workspace
                .items()
                .iter()
                .map(|item| match item {
                    Item::Tab(_) => self.theme.metrics.row_height,
                    Item::Group(group) => {
                        self.theme.metrics.row_height
                            * (1.0
                                + if group.collapsed() {
                                    0.0
                                } else {
                                    group.tabs().len() as f32
                                })
                    }
                })
                .sum();
            if new_tab_fits_after_tabs(
                content_height,
                ui.available_height(),
                reserved,
                self.theme.metrics.row_height,
            ) {
                new_tab_inside = true;
                reserved -= self.theme.metrics.row_height;
            }
        }
        let list_height = tab_list_height(ui.available_height(), reserved);
        egui::ScrollArea::vertical()
            .id_salt("browser_tabs")
            .auto_shrink([false, false])
            .max_height(list_height)
            .min_scrolled_height(list_height)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, item) in workspace.items().iter().enumerate() {
                    match item {
                        Item::Tab(id) => self.tab_row(
                            ui,
                            browser,
                            *id,
                            DropTarget::Item(index),
                            0.0,
                            palette,
                            actions,
                        ),
                        Item::Group(group) => {
                            self.group(ui, browser, group, index, palette, actions)
                        }
                    }
                }
                if new_tab_inside {
                    self.new_tab_row(ui, workspace.items().len(), palette, actions);
                }
            });
        new_tab_inside
    }

    fn new_tab_row(
        &mut self,
        ui: &mut egui::Ui,
        item_count: usize,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let (row, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), self.theme.metrics.row_height),
            Sense::click(),
        );
        let icon_x = if self.right {
            row.right() - 24.0
        } else {
            row.left() + 8.0
        };
        crate::icons::plus(
            ui.painter(),
            egui::Rect::from_min_size(
                egui::pos2(icon_x, row.center().y - 8.0),
                egui::vec2(16.0, 16.0),
            ),
            palette.secondary,
        );
        ui.painter().text(
            if self.right {
                row.right_center() - egui::vec2(32.0, 0.0)
            } else {
                row.left_center() + egui::vec2(32.0, 0.0)
            },
            if self.right {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            },
            "New Tab",
            crate::fonts::id(ui.ctx(), "Body"),
            palette.secondary,
        );
        ui.painter().text(
            if self.right {
                row.left_center() + egui::vec2(8.0, 0.0)
            } else {
                row.right_center() - egui::vec2(8.0, 0.0)
            },
            if self.right {
                egui::Align2::LEFT_CENTER
            } else {
                egui::Align2::RIGHT_CENTER
            },
            if cfg!(target_os = "macos") {
                "\u{2318}T"
            } else {
                "Ctrl+T"
            },
            crate::fonts::id(ui.ctx(), "Caption"),
            palette.tertiary,
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "New tab"));
        crate::icons::focus_ring(
            ui,
            &response,
            row,
            self.theme.accent,
            self.theme.metrics.row_radius,
        );
        if response.clicked() {
            actions.push(Action::OpenBar(Mode::NewTab));
        }
        self.drop(
            ui,
            &response,
            DropTarget::Item(item_count),
            palette,
            actions,
        );
    }

    pub(crate) fn popup_open(&self) -> bool {
        self.menu.is_some()
    }

    pub(crate) fn menu_position(&self) -> Option<egui::Pos2> {
        match self.menu? {
            Menu::Tab(_, pos) | Menu::Group(_, pos) => Some(pos),
        }
    }

    pub(crate) fn close_menu(&mut self) {
        self.menu = None;
    }

    pub(crate) fn open_tab_menu(&mut self, id: TabId, pos: egui::Pos2) {
        self.menu = Some(Menu::Tab(id, pos));
    }

    pub(crate) fn open_group_menu(&mut self, id: GroupId, pos: egui::Pos2) {
        self.menu = Some(Menu::Group(id, pos));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_drop_halves() {
        let id = GroupId::from_raw(7);
        assert_eq!(
            drop_location(DropTarget::Item(3), false),
            Location::Items(3)
        );
        assert_eq!(drop_location(DropTarget::Item(3), true), Location::Items(4));
        assert_eq!(
            drop_location(DropTarget::Group(id, 2), true),
            Location::Group(id, 3)
        );
        assert_eq!(
            drop_location(DropTarget::Pinned(0), false),
            Location::Pinned(0)
        );
    }

    #[test]
    fn avatar_is_deterministic() {
        assert_eq!(avatar("example.com"), avatar("example.com"));
        assert_eq!(avatar("example.com").0, 'E');
        assert!(avatar("example.com").1 < 8);
    }

    #[test]
    fn sidebar_has_unique_ids_and_active_row_paint() {
        for mode in [aegis_core::ThemeMode::Light, aegis_core::ThemeMode::Dark] {
            let ctx = egui::Context::default();
            ctx.options_mut(|options| options.warn_on_id_clash = true);
            let settings = aegis_core::Settings {
                theme: mode,
                ..aegis_core::Settings::default()
            };
            let mut browser = Browser::new();
            let _ = browser.apply(Command::SetSettings(settings.clone()), 0);
            let _ = browser.apply(
                Command::OpenTab {
                    url: "https://example.com".to_owned(),
                    placement: aegis_core::Placement::End,
                    activate: true,
                    opener: None,
                },
                0,
            );
            let theme = theme::resolve(&settings, mode == aegis_core::ThemeMode::Dark);
            let mut sidebar = Sidebar::new();
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(272.0, 800.0),
                        )),
                        ..egui::RawInput::default()
                    },
                    |ui| {
                        ui.set_width(272.0);
                        let _ = sidebar.show(ui, &browser, false, None, 0, false);
                    },
                );
                assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == theme.palette.row_on)));
                let capsule_bottom = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.fill == theme.palette.capsule && rect.rect.height() >= 38.0 =>
                        {
                            Some(rect.rect.bottom())
                        }
                        _ => None,
                    })
                    .max_by(|a, b| a.total_cmp(b));
                for clipped in &output.shapes {
                    if let egui::Shape::Text(text) = &clipped.shape {
                        let value = text.galley.text();
                        assert!(
                            !(value.contains("use of") && value.contains(" ID")),
                            "{value}"
                        );
                    }
                }
                output.textures_delta.clear();
                assert!(
                    capsule_bottom
                        .is_some_and(|bottom| (bottom - (800.0 - PANE_PADDING)).abs() < 1.0),
                    "{capsule_bottom:?}"
                );
            }
        }
    }

    #[test]
    fn tabs_take_height_left_by_trailing_sections() {
        assert_eq!(tab_list_height(700.0, 46.0), 654.0);
        assert_eq!(tab_list_height(700.0, 46.0 + 42.0), 612.0);
        assert_eq!(tab_list_height(100.0, 90.0), 40.0);
        assert_eq!(
            section_gap_after(SectionKind::Tabs, Some(SectionKind::NewTab)),
            0.0
        );
        assert_eq!(
            section_gap_after(SectionKind::Tabs, Some(SectionKind::Capsule)),
            8.0
        );
        let settings = aegis_core::Settings::default();
        assert_eq!(pinned_section_height(&settings, 272.0, 5), 60.0);
        assert_eq!(pinned_section_height(&settings, 272.0, 10), 104.0);
        assert!(new_tab_fits_after_tabs(68.0, 500.0, 88.0, 34.0));
        assert!(!new_tab_fits_after_tabs(500.0, 500.0, 88.0, 34.0));
    }
}
