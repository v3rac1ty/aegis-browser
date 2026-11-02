use super::*;

impl Sidebar {
    pub(super) fn spaces(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        actions: &mut Vec<Action>,
    ) {
        let spaces = browser.workspaces();
        if spaces.is_empty() {
            return;
        }
        let style = browser.settings().spaces_style;
        if style != aegis_core::SpacesStyle::Segmented {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let item_width = if style == aegis_core::SpacesStyle::Dots {
                    20.0
                } else {
                    28.0
                };
                let total =
                    item_width * spaces.len() as f32 + 8.0 * spaces.len().saturating_sub(1) as f32;
                ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                for space in spaces {
                    let selected = space.id() == browser.active_workspace();
                    let size = if style == aegis_core::SpacesStyle::Dots {
                        20.0
                    } else {
                        28.0
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(size, 28.0), Sense::click());
                    let color = theme::group_color(space.color());
                    if style == aegis_core::SpacesStyle::Dots {
                        ui.painter().circle_filled(
                            rect.center(),
                            3.5,
                            if selected {
                                color
                            } else {
                                color.gamma_multiply(0.35)
                            },
                        );
                    } else {
                        ui.painter().rect_filled(
                            rect.shrink2(egui::vec2(2.0, 2.0)),
                            5.0,
                            if selected {
                                color
                            } else {
                                self.theme.palette.well
                            },
                        );
                        let initial = space.name().chars().next().unwrap_or('?');
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            initial,
                            crate::fonts::id(ui.ctx(), "Label"),
                            if selected {
                                Color32::WHITE
                            } else {
                                self.theme.palette.text
                            },
                        );
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, space.name())
                    });
                    if response.clicked() {
                        actions.push(Action::Command(Command::SwitchWorkspace(space.id())));
                    }
                }
                let add = crate::icons::icon_button(ui, crate::icons::plus, 28.0, "Add space");
                if add.clicked() {
                    let count = spaces.len();
                    actions.push(Action::Command(Command::AddWorkspace {
                        name: format!("Space {}", count + 1),
                        color: theme::GROUP_COLORS[count % theme::GROUP_COLORS.len()],
                    }));
                }
            });
            return;
        }
        let palette = self.theme.palette;
        let width = ui.available_width();
        let height = 32.0;
        let segment_width = ((width - 36.0) / spaces.len().min(3) as f32).max(72.0);
        let track_width = segment_width * spaces.len() as f32 + 36.0;
        egui::ScrollArea::horizontal()
            .id_salt("browser_spaces")
            .show(ui, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(track_width, height), Sense::hover());
                let segment_offset = if self.right { 30.0 } else { 0.0 };
                ui.painter().rect_filled(
                    rect,
                    self.theme.metrics.control_radius + 2.0,
                    palette.well,
                );
                let selected = spaces
                    .iter()
                    .position(|space| space.id() == browser.active_workspace())
                    .unwrap_or(0);
                let visual_index = if self.right {
                    spaces.len() - 1 - selected
                } else {
                    selected
                };
                let position = self.motion.value(
                    ui.ctx(),
                    egui::Id::unique("spaces_thumb"),
                    visual_index as f32,
                    0.20,
                    self.frame_dt,
                    browser.settings().motion,
                );
                let thumb = egui::Rect::from_min_size(
                    rect.min + egui::vec2(3.0 + segment_offset + position * segment_width, 3.0),
                    egui::vec2(segment_width, 26.0),
                );
                ui.painter()
                    .rect_filled(thumb, self.theme.metrics.control_radius, palette.seg_on);
                for (index, space) in spaces.iter().enumerate() {
                    let visual_index = if self.right {
                        spaces.len() - 1 - index
                    } else {
                        index
                    };
                    let segment = egui::Rect::from_min_size(
                        rect.min
                            + egui::vec2(
                                3.0 + segment_offset + visual_index as f32 * segment_width,
                                3.0,
                            ),
                        egui::vec2(segment_width, 26.0),
                    );
                    let segment_id = ui.scope_id().with(space.id().get());
                    let response = crate::widgets::segment(
                        ui,
                        segment,
                        segment_id,
                        space.name(),
                        theme::group_color(space.color()),
                        self.theme,
                    );
                    if response.clicked() {
                        actions.push(Action::Command(Command::SwitchWorkspace(space.id())));
                    }
                }
                let add = egui::Rect::from_min_size(
                    egui::pos2(
                        if self.right {
                            rect.left() + 3.0
                        } else {
                            rect.right() - 29.0
                        },
                        rect.top() + 3.0,
                    ),
                    egui::vec2(26.0, 26.0),
                );
                let add_response =
                    ui.interact(add, ui.scope_id().with("add_space"), Sense::click());
                crate::icons::button_background(
                    ui,
                    &add_response,
                    add,
                    palette.hover,
                    palette.seg_on,
                    self.theme.metrics.control_radius,
                );
                crate::icons::plus(ui.painter(), add.shrink(5.0), palette.secondary);
                add_response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "Add space")
                });
                crate::icons::focus_ring(
                    ui,
                    &add_response,
                    add,
                    self.theme.accent,
                    self.theme.metrics.control_radius,
                );
                if add_response.clicked() {
                    let color = theme::GROUP_COLORS[spaces.len() % theme::GROUP_COLORS.len()];
                    actions.push(Action::Command(Command::AddWorkspace {
                        name: format!("Space {}", spaces.len() + 1),
                        color,
                    }));
                }
            });
    }
}
