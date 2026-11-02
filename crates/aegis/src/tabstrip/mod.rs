mod tabs;
mod toolbar;

use std::time::{Duration, Instant};

use aegis_core::{Browser, Command, GroupId, Location, TabId};
use eframe::egui;

use crate::motion::Motion;
use crate::sidebar::{Action, Sidebar};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Pinned(usize),
    Item(usize),
    Group(GroupId, usize),
}

pub fn drop_location(target: DropTarget, after: bool, into_group: bool) -> Location {
    match target {
        DropTarget::Pinned(index) => Location::Pinned(index + usize::from(after)),
        DropTarget::Item(index) => Location::Items(index + usize::from(after)),
        DropTarget::Group(id, index) if into_group => Location::Group(id, index),
        DropTarget::Group(id, index) => Location::Group(id, index + usize::from(after)),
    }
}

pub fn scroll_into_view(offset: f32, viewport: f32, start: f32, end: f32) -> f32 {
    if start < offset {
        start.max(0.0)
    } else if end > offset + viewport {
        (end - viewport).max(0.0)
    } else {
        offset
    }
}

pub struct TabStrip {
    motion: Motion,
    scroll: f32,
    last_active: Option<TabId>,
    privacy_open: bool,
    privacy_anchor: Option<egui::Pos2>,
    space_open: Option<egui::Pos2>,
    menu_seen_focused: bool,
    space_seen_focused: bool,
    privacy_seen_focused: bool,
    frame_dt: f32,
}

pub struct CompactTop {
    open: bool,
    left_at: Option<Instant>,
    motion: Motion,
}

impl CompactTop {
    pub fn new() -> Self {
        let mut motion = Motion::new();
        motion.seed(egui::Id::unique("classic_top_reveal"), 0.0);
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
        let hovered = ui
            .interact(
                rect,
                ui.scope_id().with("classic_hot_strip"),
                egui::Sense::hover(),
            )
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
        tabstrip: &mut TabStrip,
        browser: &Browser,
        sidebar: &mut Sidebar,
        loading: bool,
        blocked: u32,
    ) -> Vec<Action> {
        let dt = self.motion.frame();
        let progress = self.motion.value(
            ctx,
            egui::Id::unique("classic_top_reveal"),
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
        let popup_open =
            sidebar.popup_open() || tabstrip.privacy_open || tabstrip.space_open.is_some();
        let height = if popup_open {
            390.0
        } else {
            (76.0 * progress).max(1.0)
        };
        if progress <= 0.0 && !popup_open {
            return Vec::new();
        }
        let builder = egui::ViewportBuilder::default()
            .with_icon(crate::logo::icon())
            .with_position(egui::pos2(
                window.left(),
                window.top() + theme::HOT_STRIP_WIDTH,
            ))
            .with_inner_size([window.width(), height])
            .with_transparent(true)
            .with_has_shadow(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false);
        let (hovered, actions) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("classic_top_overlay"),
            builder,
            |ui, _| {
                let mut actions = Vec::new();
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let content = egui::Rect::from_min_size(
                            ui.max_rect().min
                                + egui::vec2(
                                    0.0,
                                    if popup_open {
                                        -76.0 * (1.0 - progress)
                                    } else {
                                        height - 76.0
                                    },
                                ),
                            egui::vec2(ui.max_rect().width(), 76.0),
                        );
                        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                            actions = tabstrip.show(ui, browser, sidebar, loading, blocked);
                        });
                    });
                (
                    ui.input(|input| {
                        input
                            .pointer
                            .hover_pos()
                            .is_some_and(|pos| popup_open || pos.y <= 76.0 * progress)
                    }),
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

impl TabStrip {
    pub fn new() -> Self {
        Self {
            motion: Motion::new(),
            scroll: 0.0,
            last_active: None,
            privacy_open: false,
            privacy_anchor: None,
            space_open: None,
            menu_seen_focused: false,
            space_seen_focused: false,
            privacy_seen_focused: false,
            frame_dt: 0.0,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        sidebar: &mut Sidebar,
        loading: bool,
        blocked: u32,
    ) -> Vec<Action> {
        self.frame_dt = self.motion.frame();
        let appearance = theme::resolve(
            browser.settings(),
            ui.ctx().system_theme() == Some(egui::Theme::Dark),
        );
        let mut actions = Vec::new();
        let menu_was_open = sidebar.popup_open();
        let space_was_open = self.space_open.is_some();
        let privacy_was_open = self.privacy_open;
        let (row, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
        ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
            self.tabs(ui, browser, sidebar, appearance, &mut actions);
        });
        let (row, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::hover());
        ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
            self.toolbar(
                ui,
                browser,
                sidebar,
                loading,
                blocked,
                appearance,
                &mut actions,
            );
        });
        if let Some(pos) = sidebar.menu_position() {
            let origin = ui
                .ctx()
                .input(|input| input.viewport().outer_rect)
                .map_or(egui::Pos2::ZERO, |rect| rect.min);
            let builder = egui::ViewportBuilder::default()
                .with_icon(crate::logo::icon())
                .with_position(origin + pos.to_vec2())
                .with_inner_size([210.0, 240.0])
                .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
                .with_has_shadow(false)
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_always_on_top()
                .with_active(true);
            let (menu_actions, focused) = ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("classic_context_menu"),
                builder,
                |ctx, _| {
                    let mut menu_actions = Vec::new();
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            sidebar.menu_at(
                                ui,
                                browser,
                                appearance.palette,
                                &mut menu_actions,
                                Some(egui::pos2(4.0, 4.0)),
                            );
                        });
                    (menu_actions, ctx.input(|input| input.focused))
                },
            );
            actions.extend(menu_actions);
            self.menu_seen_focused |= focused;
            if (self.menu_seen_focused && !focused)
                || ui.input(|input| {
                    input.key_pressed(egui::Key::Escape) || input.pointer.any_pressed()
                }) && menu_was_open
            {
                sidebar.close_menu();
                self.menu_seen_focused = false;
            }
        }
        if let Some(pos) = self.space_open {
            let origin = ui
                .ctx()
                .input(|input| input.viewport().outer_rect)
                .map_or(egui::Pos2::ZERO, |rect| rect.min);
            let builder = egui::ViewportBuilder::default()
                .with_icon(crate::logo::icon())
                .with_position(origin + pos.to_vec2())
                .with_inner_size([190.0, 260.0])
                .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
                .with_has_shadow(false)
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_always_on_top()
                .with_active(true);
            let (menu_actions, focused, close) = ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("classic_spaces_menu"),
                builder,
                |ctx, _| {
                    let mut menu_actions = Vec::new();
                    let mut close = false;
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(appearance.palette.overlay)
                                .show(ui, |ui| {
                                    ui.set_min_width(170.0);
                                    for space in browser.workspaces() {
                                        if ui.button(space.name()).clicked() {
                                            menu_actions.push(Action::Command(
                                                Command::SwitchWorkspace(space.id()),
                                            ));
                                            close = true;
                                        }
                                    }
                                    if ui.button("New space").clicked() {
                                        let count = browser.workspaces().len();
                                        menu_actions.push(Action::Command(Command::AddWorkspace {
                                            name: format!("Space {}", count + 1),
                                            color: theme::GROUP_COLORS
                                                [count % theme::GROUP_COLORS.len()],
                                        }));
                                        close = true;
                                    }
                                });
                        });
                    (menu_actions, ctx.input(|input| input.focused), close)
                },
            );
            actions.extend(menu_actions);
            self.space_seen_focused |= focused;
            if close
                || (self.space_seen_focused && !focused)
                || ui.input(|input| {
                    input.key_pressed(egui::Key::Escape) || input.pointer.any_pressed()
                }) && space_was_open
            {
                self.space_open = None;
                self.space_seen_focused = false;
            }
        }
        if self.privacy_open
            && let Some(pos) = self.privacy_anchor
        {
            let origin = ui
                .ctx()
                .input(|input| input.viewport().outer_rect)
                .map_or(egui::Pos2::ZERO, |rect| rect.min);
            let builder = egui::ViewportBuilder::default()
                .with_icon(crate::logo::icon())
                .with_position(origin + pos.to_vec2())
                .with_inner_size([260.0, 300.0])
                .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
                .with_has_shadow(false)
                .with_decorations(false)
                .with_resizable(false)
                .with_taskbar(false)
                .with_always_on_top()
                .with_active(true);
            let focused = ui.ctx().show_viewport_immediate(
                egui::ViewportId::from_hash_of("classic_privacy_menu"),
                builder,
                |ctx, _| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(appearance.palette.overlay)
                                .show(ui, |ui| {
                                    let host = browser
                                        .active_tab()
                                        .and_then(|id| browser.tab(id))
                                        .map_or_else(
                                            || "this page".to_owned(),
                                            |tab| crate::sidebar::display_host(tab.url()),
                                        );
                                    crate::protections::show(ui, appearance, &host, blocked);
                                });
                        });
                    ctx.input(|input| input.focused)
                },
            );
            self.privacy_seen_focused |= focused;
            if (self.privacy_seen_focused && !focused)
                || ui.input(|input| {
                    input.key_pressed(egui::Key::Escape)
                        || (privacy_was_open && input.pointer.any_pressed())
                })
            {
                self.privacy_open = false;
                self.privacy_seen_focused = false;
            }
        }
        actions
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn drop_halves_and_group_and_pinned() {
        let group = GroupId::from_raw(4);
        assert_eq!(
            drop_location(DropTarget::Item(2), false, false),
            Location::Items(2)
        );
        assert_eq!(
            drop_location(DropTarget::Item(2), true, false),
            Location::Items(3)
        );
        assert_eq!(
            drop_location(DropTarget::Pinned(1), true, false),
            Location::Pinned(2)
        );
        assert_eq!(
            drop_location(DropTarget::Group(group, 1), false, true),
            Location::Group(group, 1)
        );
        assert_eq!(
            drop_location(DropTarget::Group(group, 1), true, false),
            Location::Group(group, 2)
        );
    }

    #[test]
    fn scrolls_only_when_tab_is_outside_view() {
        assert_eq!(scroll_into_view(50.0, 200.0, 100.0, 180.0), 50.0);
        assert_eq!(scroll_into_view(50.0, 200.0, 20.0, 80.0), 20.0);
        assert_eq!(scroll_into_view(50.0, 200.0, 230.0, 300.0), 100.0);
        assert_eq!(scroll_into_view(0.0, 200.0, 0.0, 100.0), 0.0);
    }

    #[test]
    fn same_list_moves_adjust_for_removal() {
        use crate::sidebar::location_after_removal;
        use aegis_core::{Command, Item, Placement};

        let mut browser = Browser::new();
        let mut ids = Vec::new();
        for name in ["a", "b", "c"] {
            browser
                .apply(
                    Command::OpenTab {
                        url: format!("https://{name}.example"),
                        placement: Placement::End,
                        activate: true,
                        opener: None,
                    },
                    0,
                )
                .unwrap();
            ids.push(browser.active_tab().unwrap());
        }
        let mut forward = browser.clone();
        let to = location_after_removal(
            &forward,
            ids[0],
            drop_location(DropTarget::Item(1), true, false),
        );
        forward
            .apply(Command::MoveTab { tab: ids[0], to }, 0)
            .unwrap();
        let items = forward
            .workspace(forward.active_workspace())
            .unwrap()
            .items();
        assert_eq!(
            items,
            &[Item::Tab(ids[1]), Item::Tab(ids[0]), Item::Tab(ids[2])]
        );

        let to = location_after_removal(
            &browser,
            ids[2],
            drop_location(DropTarget::Item(0), false, false),
        );
        browser
            .apply(Command::MoveTab { tab: ids[2], to }, 0)
            .unwrap();
        let items = browser
            .workspace(browser.active_workspace())
            .unwrap()
            .items();
        assert_eq!(
            items,
            &[Item::Tab(ids[2]), Item::Tab(ids[0]), Item::Tab(ids[1])]
        );
    }
}
