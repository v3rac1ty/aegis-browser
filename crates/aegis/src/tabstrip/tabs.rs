use aegis_core::{Browser, Command, Group, Item, TabId};
use eframe::egui::{self, Sense, Stroke};

use super::{DropTarget, TabStrip, drop_location, scroll_into_view};
use crate::command_bar::Mode;
use crate::sidebar::{Action, Sidebar, display_host};
use crate::theme::{self, Theme};

impl TabStrip {
    pub(super) fn tabs(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        sidebar: &mut Sidebar,
        theme: Theme,
        actions: &mut Vec<Action>,
    ) {
        let row = ui.max_rect();
        ui.painter().rect_filled(row, 0.0, theme.palette.pane);
        if theme.pane_scrim.a() > 0 {
            ui.painter().rect_filled(row, 0.0, theme.pane_scrim);
        }
        let background = ui.interact(
            row,
            ui.scope_id().with("window_drag"),
            Sense::click_and_drag(),
        );
        crate::widgets::window_drag(ui, &background, row);
        let Some(workspace) = browser.workspace(browser.active_workspace()) else {
            return;
        };
        let active = browser.active_tab();
        let changed = active != self.last_active;
        let mut active_bounds = None;
        let viewport = row.width();
        let output = egui::ScrollArea::horizontal()
            .id_salt("classic_tabs_scroll")
            .horizontal_scroll_offset(self.scroll)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    #[cfg(target_os = "macos")]
                    ui.add_space(76.0);
                    #[cfg(not(target_os = "macos"))]
                    ui.add_space(8.0);
                    if workspace.pinned().is_empty() {
                        let (target, response) =
                            ui.allocate_exact_size(egui::vec2(12.0, 32.0), Sense::hover());
                        self.drop(ui, &response, DropTarget::Pinned(0), false, theme, actions);
                        if response.dnd_hover_payload::<TabId>().is_some() {
                            ui.painter().line_segment(
                                [target.left_top(), target.left_bottom()],
                                Stroke::new(2.0, theme.accent),
                            );
                        }
                    }
                    for (index, id) in workspace.pinned().iter().enumerate() {
                        let rect = self.tab(
                            ui,
                            browser,
                            *id,
                            34.0,
                            None,
                            DropTarget::Pinned(index),
                            sidebar,
                            theme,
                            actions,
                        );
                        if active == Some(*id) {
                            active_bounds = Some((rect.left(), rect.right()));
                        }
                    }
                    let (line, _) = ui.allocate_exact_size(egui::vec2(1.0, 24.0), Sense::hover());
                    ui.painter().line_segment(
                        [line.center_top(), line.center_bottom()],
                        Stroke::new(theme::hairline(ui.ctx()), theme.palette.hairline),
                    );
                    for (index, item) in workspace.items().iter().enumerate() {
                        match item {
                            Item::Tab(id) => {
                                let rect = self.tab(
                                    ui,
                                    browser,
                                    *id,
                                    196.0,
                                    None,
                                    DropTarget::Item(index),
                                    sidebar,
                                    theme,
                                    actions,
                                );
                                if active == Some(*id) {
                                    active_bounds = Some((rect.left(), rect.right()));
                                }
                            }
                            Item::Group(group) => {
                                self.group(
                                    ui,
                                    browser,
                                    group,
                                    index,
                                    sidebar,
                                    theme,
                                    actions,
                                    active,
                                    &mut active_bounds,
                                );
                            }
                        }
                    }
                    let plus = crate::icons::icon_button(ui, crate::icons::plus, 30.0, "New tab");
                    if plus.clicked() {
                        actions.push(Action::OpenBar(Mode::NewTab));
                    }
                    if let Some(tab) = plus.dnd_release_payload::<TabId>() {
                        actions.push(Action::Command(Command::MoveTab {
                            tab: *tab,
                            to: drop_location(
                                DropTarget::Item(workspace.items().len()),
                                false,
                                false,
                            ),
                        }));
                    }
                    ui.add_space(8.0);
                });
            });
        self.scroll = output.state.clamped_offset().x;
        if changed {
            if let Some((start, end)) = active_bounds {
                let start = start - row.left() + self.scroll;
                let end = end - row.left() + self.scroll;
                let next = scroll_into_view(self.scroll, viewport, start, end);
                if next != self.scroll {
                    self.scroll = next;
                    ui.ctx().request_repaint();
                }
            }
            self.last_active = active;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn group(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        group: &Group,
        item_index: usize,
        sidebar: &mut Sidebar,
        theme: Theme,
        actions: &mut Vec<Action>,
        active: Option<TabId>,
        active_bounds: &mut Option<(f32, f32)>,
    ) {
        let progress = self.motion.value(
            ui.ctx(),
            egui::Id::unique(("classic_group", group.id().get())),
            if group.collapsed() { 0.0 } else { 1.0 },
            0.24,
            self.frame_dt,
            browser.settings().motion,
        );
        let label = if group.collapsed() {
            format!("{} · {}", group.name(), group.tabs().len())
        } else {
            group.name().to_owned()
        };
        let width = (label.chars().count() as f32 * 7.0 + 24.0).max(48.0);
        let (pill, response) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::click());
        if group.collapsed() && active.is_some_and(|id| group.tabs().contains(&id)) {
            *active_bounds = Some((pill.left(), pill.right()));
        }
        let color = theme::group_color(group.color());
        ui.painter()
            .rect_filled(pill, theme.metrics.row_radius, theme.palette.well);
        ui.painter()
            .circle_filled(pill.left_center() + egui::vec2(10.0, 0.0), 3.0, color);
        ui.painter().text(
            pill.left_center() + egui::vec2(18.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            crate::fonts::id(ui.ctx(), "Label"),
            theme.palette.text,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Group {}", group.name()))
        });
        crate::icons::focus_ring(ui, &response, pill, theme.accent, theme.metrics.row_radius);
        if response.clicked() {
            actions.push(Action::Command(Command::SetGroupCollapsed {
                group: group.id(),
                collapsed: !group.collapsed(),
            }));
        }
        if response.secondary_clicked() {
            sidebar.open_group_menu(
                group.id(),
                response.hover_pos().unwrap_or(pill.left_bottom()),
            );
        }
        self.drop(
            ui,
            &response,
            DropTarget::Group(group.id(), 0),
            true,
            theme,
            actions,
        );
        let width = group.tabs().len() as f32 * 200.0 * progress;
        if width <= 0.0 {
            return;
        }
        let (area, _) = ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::hover());
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                area.min,
                egui::vec2(group.tabs().len() as f32 * 200.0, 32.0),
            )),
            |ui| {
                ui.set_clip_rect(area);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (index, id) in group.tabs().iter().enumerate() {
                        let rect = self.tab(
                            ui,
                            browser,
                            *id,
                            196.0,
                            Some(color),
                            DropTarget::Group(group.id(), index),
                            sidebar,
                            theme,
                            actions,
                        );
                        if active == Some(*id) {
                            *active_bounds = Some((rect.left(), rect.right()));
                        }
                    }
                });
            },
        );
        let _ = item_index;
    }

    #[allow(clippy::too_many_arguments)]
    fn tab(
        &self,
        ui: &mut egui::Ui,
        browser: &Browser,
        id: TabId,
        width: f32,
        group_color: Option<egui::Color32>,
        target: DropTarget,
        sidebar: &mut Sidebar,
        theme: Theme,
        actions: &mut Vec<Action>,
    ) -> egui::Rect {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 32.0), Sense::click_and_drag());
        let Some(tab) = browser.tab(id) else {
            return rect;
        };
        let active = browser.active_tab() == Some(id);
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
        if active || hover > 0.0 {
            ui.painter().rect_filled(
                rect,
                theme.metrics.row_radius,
                if active {
                    theme.palette.row_on
                } else {
                    theme.palette.hover.gamma_multiply(hover)
                },
            );
        }
        if active || group_color.is_some() {
            let color = group_color.unwrap_or(theme.accent);
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left() + 12.0, rect.bottom() - 2.0),
                    egui::pos2(rect.right() - 12.0, rect.bottom()),
                ),
                1.0,
                color,
            );
        }
        let host = display_host(tab.url());
        if browser.settings().tab_icons != aegis_core::TabIcons::Off {
            crate::widgets::avatar(
                ui,
                egui::Rect::from_center_size(
                    rect.left_center() + egui::vec2(16.0, 0.0),
                    egui::vec2(18.0, 18.0),
                ),
                &host,
                theme,
                browser.settings().tab_icons == aegis_core::TabIcons::Circles,
            );
        }
        if width > 40.0 {
            let title = if tab.title().is_empty() {
                &host
            } else {
                tab.title()
            };
            let title: String = title
                .chars()
                .take(if response.hovered() { 18 } else { 22 })
                .collect();
            ui.painter()
                .with_clip_rect(rect.shrink2(egui::vec2(32.0, 0.0)))
                .text(
                    rect.left_center() + egui::vec2(32.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    title,
                    crate::fonts::id(ui.ctx(), "Body"),
                    theme.palette.text,
                );
            if active || response.hovered() {
                let close = egui::Rect::from_center_size(
                    rect.right_center() - egui::vec2(15.0, 0.0),
                    egui::vec2(20.0, 20.0),
                );
                let button = ui.interact(
                    close,
                    ui.scope_id().with(("close", id.get())),
                    Sense::click(),
                );
                crate::icons::close(ui.painter(), close.shrink(4.0), theme.palette.secondary);
                if button.clicked() {
                    actions.push(Action::Command(Command::CloseTab(id)));
                }
            }
        }
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tab.title()));
        crate::icons::focus_ring(ui, &response, rect, theme.accent, theme.metrics.row_radius);
        if response.clicked() {
            actions.push(Action::Command(Command::ActivateTab(id)));
        }
        if response.middle_clicked() {
            actions.push(Action::Command(Command::CloseTab(id)));
        }
        if response.secondary_clicked() {
            sidebar.open_tab_menu(id, response.hover_pos().unwrap_or(rect.left_bottom()));
        }
        response.dnd_set_drag_payload(id);
        self.drop(ui, &response, target, false, theme, actions);
        rect
    }

    fn drop(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        target: DropTarget,
        into_group: bool,
        theme: Theme,
        actions: &mut Vec<Action>,
    ) {
        let after = response
            .hover_pos()
            .is_some_and(|pos| pos.x >= response.rect.center().x);
        if response.dnd_hover_payload::<TabId>().is_some() {
            let x = if after {
                response.rect.right()
            } else {
                response.rect.left()
            };
            ui.painter().line_segment(
                [
                    egui::pos2(x, response.rect.top() + 2.0),
                    egui::pos2(x, response.rect.bottom() - 2.0),
                ],
                Stroke::new(2.0, theme.accent),
            );
        }
        if let Some(tab) = response.dnd_release_payload::<TabId>() {
            actions.push(Action::Command(Command::MoveTab {
                tab: *tab,
                to: drop_location(target, after, into_group),
            }));
        }
    }
}
