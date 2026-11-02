use super::*;

impl Sidebar {
    pub(super) fn pinned(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        pinned: &[TabId],
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let per_row = browser.settings().pinned_per_row as usize;
        let width = ui.available_width();
        let size = ((width - 8.0 * (per_row as f32 - 1.0)) / per_row as f32).min(
            if browser.settings().pinned_style == aegis_core::PinnedStyle::Tiles {
                44.0
            } else {
                40.0
            },
        );
        if size < 17.0 {
            return;
        }
        for (row_index, chunk) in pinned.chunks(per_row).enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if self.right {
                    ui.add_space(
                        (ui.available_width() - chunk.len() as f32 * (size + 8.0)).max(0.0),
                    );
                }
                for (column, id) in chunk.iter().enumerate() {
                    let Some(tab) = browser.tab(*id) else {
                        continue;
                    };
                    let index = row_index * per_row + column;
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(size, size), Sense::click_and_drag());
                    let active = browser.active_tab() == Some(*id);
                    let hover = ui.ctx().animate_bool_with_time(
                        response.id.with("hover"),
                        response.hovered(),
                        0.12,
                    );
                    let fill = if active {
                        palette.row_on
                    } else if hover > 0.0 {
                        palette.hover.gamma_multiply(hover)
                    } else {
                        palette.pane
                    };
                    let radius =
                        if browser.settings().pinned_style == aegis_core::PinnedStyle::Tiles {
                            12.0
                        } else {
                            size / 2.0
                        };
                    ui.painter().rect_filled(rect, radius, fill);
                    if active {
                        ui.painter().rect_stroke(
                            crate::widgets::safe_shrink(rect, 1.0),
                            radius,
                            Stroke::new(2.0, self.theme.accent),
                            StrokeKind::Inside,
                        );
                        ui.painter().rect_stroke(
                            crate::widgets::safe_shrink(rect, 3.0),
                            radius - 3.0,
                            Stroke::new(2.0, palette.page),
                            StrokeKind::Inside,
                        );
                    }
                    if browser.settings().tab_icons != aegis_core::TabIcons::Off {
                        crate::widgets::avatar(
                            ui,
                            crate::widgets::safe_shrink(rect, 8.0),
                            &display_host(tab.url()),
                            self.theme,
                            browser.settings().tab_icons == aegis_core::TabIcons::Circles,
                        );
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::Button,
                            true,
                            if tab.title().is_empty() {
                                display_host(tab.url())
                            } else {
                                tab.title().to_owned()
                            },
                        )
                    });
                    crate::icons::focus_ring(ui, &response, rect, self.theme.accent, radius);
                    if response.clicked() {
                        actions.push(Action::Command(Command::ActivateTab(*id)));
                    }
                    if response.middle_clicked() {
                        actions.push(Action::Command(Command::CloseTab(*id)));
                    }
                    if response.secondary_clicked() {
                        self.menu = Some(Menu::Tab(
                            *id,
                            response.hover_pos().unwrap_or(rect.left_bottom()),
                        ));
                    }
                    response.dnd_set_drag_payload(*id);
                    self.drop(ui, &response, DropTarget::Pinned(index), palette, actions);
                }
            });
            ui.add_space(4.0);
        }
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), Sense::hover());
        self.drop(
            ui,
            &response,
            DropTarget::Pinned(pinned.len()),
            palette,
            actions,
        );
        if response.dnd_hover_payload::<TabId>().is_some() {
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(2.0, palette.text),
            );
        }
    }

    pub(super) fn group(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        group: &Group,
        item_index: usize,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), self.theme.metrics.row_height),
            Sense::click(),
        );
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
        if hover > 0.0 {
            ui.painter().rect_filled(
                rect,
                self.theme.metrics.row_radius,
                palette.hover.gamma_multiply(hover),
            );
        }
        ui.painter().circle_filled(
            egui::pos2(
                if self.right {
                    rect.right() - 13.0
                } else {
                    rect.left() + 13.0
                },
                rect.center().y,
            ),
            4.0,
            theme::group_color(group.color()),
        );
        let progress = self.motion.value(
            ui.ctx(),
            egui::Id::unique(("group", group.id().get())),
            if group.collapsed() { 0.0 } else { 1.0 },
            0.24,
            self.frame_dt,
            browser.settings().motion,
        );
        let badge = egui::Rect::from_center_size(
            if self.right {
                rect.left_center() + egui::vec2(34.0, 0.0)
            } else {
                rect.right_center() - egui::vec2(34.0, 0.0)
            },
            egui::vec2(24.0, 18.0),
        );
        let [r, g, b, _] = theme::group_color(group.color()).to_array();
        ui.painter()
            .rect_filled(badge, 9.0, Color32::from_rgba_unmultiplied(r, g, b, 30));
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            group.tabs().len().to_string(),
            crate::fonts::id(ui.ctx(), "Caption"),
            theme::group_color(group.color()),
        );
        if self
            .rename
            .as_ref()
            .is_some_and(|(id, _)| *id == group.id())
        {
            if let Some((_, text)) = &mut self.rename {
                let edit = ui.put(
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(26.0, 2.0),
                        egui::vec2(rect.width() - 50.0, 28.0),
                    ),
                    egui::TextEdit::singleline(text),
                );
                edit.request_focus();
                if edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    actions.push(Action::Command(Command::RenameGroup {
                        group: group.id(),
                        name: text.clone(),
                    }));
                    self.rename = None;
                } else if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    self.rename = None;
                }
            }
        } else {
            ui.painter().text(
                if self.right {
                    rect.right_center() - egui::vec2(26.0, 0.0)
                } else {
                    rect.left_center() + egui::vec2(26.0, 0.0)
                },
                if self.right {
                    egui::Align2::RIGHT_CENTER
                } else {
                    egui::Align2::LEFT_CENTER
                },
                group.name(),
                crate::fonts::id(ui.ctx(), "Body"),
                if group.collapsed() {
                    palette.secondary
                } else {
                    palette.text
                },
            );
            paint_chevron(
                ui,
                if self.right {
                    rect.left_center() + egui::vec2(14.0, 0.0)
                } else {
                    rect.right_center() - egui::vec2(14.0, 0.0)
                },
                progress,
                palette.secondary,
            );
        }
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                true,
                format!("Group {}, {} tabs", group.name(), group.tabs().len()),
            )
        });
        crate::icons::focus_ring(
            ui,
            &response,
            rect,
            self.theme.accent,
            self.theme.metrics.row_radius,
        );
        if response.clicked() && self.rename.is_none() {
            actions.push(Action::Command(Command::SetGroupCollapsed {
                group: group.id(),
                collapsed: !group.collapsed(),
            }));
        }
        if response.secondary_clicked() {
            self.menu = Some(Menu::Group(
                group.id(),
                response.hover_pos().unwrap_or(rect.left_bottom()),
            ));
        }
        self.drop(
            ui,
            &response,
            DropTarget::Group(group.id(), 0),
            palette,
            actions,
        );
        let full_height = group.tabs().len() as f32 * self.theme.metrics.row_height;
        let visible_height = full_height * progress;
        if visible_height > 0.0 {
            let (well, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), visible_height),
                Sense::hover(),
            );
            let [r, g, b, _] = theme::group_color(group.color()).to_array();
            ui.painter().rect_filled(
                well,
                self.theme.metrics.row_radius + 2.0,
                Color32::from_rgba_unmultiplied(r, g, b, 20),
            );
            ui.painter().rect_stroke(
                well,
                self.theme.metrics.row_radius + 2.0,
                Stroke::new(
                    theme::hairline(ui.ctx()),
                    Color32::from_rgba_unmultiplied(r, g, b, 41),
                ),
                StrokeKind::Inside,
            );
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                    well.min,
                    egui::vec2(well.width(), full_height),
                )),
                |ui| {
                    ui.set_clip_rect(well);
                    ui.set_opacity(progress);
                    for (index, id) in group.tabs().iter().enumerate() {
                        self.tab_row(
                            ui,
                            browser,
                            *id,
                            DropTarget::Group(group.id(), index),
                            12.0,
                            palette,
                            actions,
                        );
                    }
                },
            );
        }
        let _ = item_index;
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn tab_row(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        id: TabId,
        target: DropTarget,
        indent: f32,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let Some(tab) = browser.tab(id) else {
            return;
        };
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), self.theme.metrics.row_height),
            Sense::click_and_drag(),
        );
        let active = browser.active_tab() == Some(id);
        let hover =
            ui.ctx()
                .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
        if active || hover > 0.0 {
            if active {
                let shadow = egui::Shadow {
                    offset: [0, 1],
                    blur: 3,
                    spread: 0,
                    color: if self.theme.dark {
                        Color32::from_black_alpha(30)
                    } else {
                        Color32::from_black_alpha(26)
                    },
                };
                ui.painter()
                    .add(shadow.as_shape(rect, self.theme.metrics.row_radius));
            }
            ui.painter().rect_filled(
                rect,
                self.theme.metrics.row_radius,
                if active {
                    palette.row_on
                } else {
                    palette.hover.gamma_multiply(hover)
                },
            );
        }
        if active {
            let marker_x = if self.right {
                rect.right() - 6.0
            } else {
                rect.left() + 3.0
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(marker_x, rect.center().y - 7.0),
                    egui::vec2(3.0, 14.0),
                ),
                2.0,
                self.theme.accent,
            );
            self.row_edge(ui, rect);
        }
        let host = display_host(tab.url());
        let icons = browser.settings().tab_icons;
        if icons != aegis_core::TabIcons::Off {
            let avatar = egui::Rect::from_min_size(
                egui::pos2(
                    if self.right {
                        rect.right() - 28.0 - indent
                    } else {
                        rect.left() + 12.0 + indent
                    },
                    rect.center().y - 8.0,
                ),
                egui::vec2(16.0, 16.0),
            );
            crate::widgets::avatar(
                ui,
                avatar,
                &host,
                self.theme,
                icons == aegis_core::TabIcons::Circles,
            );
        }
        let title = if tab.title().is_empty() {
            &host
        } else {
            tab.title()
        };
        let max_chars = if active || response.hovered() { 22 } else { 26 };
        let label = match browser.settings().tab_label {
            aegis_core::TabLabel::Title => truncate(title, max_chars),
            aegis_core::TabLabel::Site => truncate(&host, max_chars),
            aegis_core::TabLabel::Both => truncate_both(title, &host, max_chars),
        };
        let label_offset = if icons == aegis_core::TabIcons::Off {
            12.0
        } else {
            38.0
        };
        ui.painter().text(
            if self.right {
                rect.right_center() - egui::vec2(label_offset + indent, 0.0)
            } else {
                rect.left_center() + egui::vec2(label_offset + indent, 0.0)
            },
            if self.right {
                egui::Align2::RIGHT_CENTER
            } else {
                egui::Align2::LEFT_CENTER
            },
            label,
            if active {
                egui::FontId::new(
                    13.0 * f32::from(browser.settings().text_scale) / 100.0,
                    egui::FontFamily::Name("semibold".into()),
                )
            } else {
                crate::fonts::id(ui.ctx(), "Body")
            },
            palette.text,
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, title));
        crate::icons::focus_ring(
            ui,
            &response,
            rect,
            self.theme.accent,
            self.theme.metrics.row_radius,
        );
        if response.clicked() {
            actions.push(Action::Command(Command::ActivateTab(id)));
        }
        if response.middle_clicked() {
            actions.push(Action::Command(Command::CloseTab(id)));
        }
        if response.secondary_clicked() {
            self.menu = Some(Menu::Tab(
                id,
                response.hover_pos().unwrap_or(rect.left_bottom()),
            ));
        }
        if match browser.settings().close_button {
            aegis_core::CloseButton::Always => true,
            aegis_core::CloseButton::OnHover => active || response.hovered(),
            aegis_core::CloseButton::Never => false,
        } {
            let close_rect = egui::Rect::from_center_size(
                if self.right {
                    rect.left_center() + egui::vec2(18.0, 0.0)
                } else {
                    rect.right_center() - egui::vec2(18.0, 0.0)
                },
                egui::vec2(20.0, 20.0),
            );
            let close = ui.interact(
                close_rect,
                ui.scope_id().with(("close", id.get())),
                Sense::click(),
            );
            crate::icons::button_background(
                ui,
                &close,
                close_rect,
                palette.hover,
                palette.seg_on,
                6.0,
            );
            crate::icons::close(ui.painter(), close_rect.shrink(4.0), palette.secondary);
            close.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Close {title}"))
            });
            crate::icons::focus_ring(ui, &close, close_rect, self.theme.accent, 6.0);
            if close.clicked() {
                actions.push(Action::Command(Command::CloseTab(id)));
            }
        }
        response.dnd_set_drag_payload(id);
        self.drop(ui, &response, target, palette, actions);
    }

    // dark rows only get an inset top highlight, light rows a thin white edge
    fn row_edge(&self, ui: &egui::Ui, rect: egui::Rect) {
        let radius = self.theme.metrics.row_radius;
        let width = theme::hairline(ui.ctx());
        if self.theme.dark {
            let y = rect.top() + width * 0.5;
            ui.painter().line_segment(
                [
                    egui::pos2(rect.left() + radius, y),
                    egui::pos2(rect.right() - radius, y),
                ],
                Stroke::new(width, Color32::from_white_alpha(20)),
            );
            return;
        }
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(width, self.theme.palette.pane_highlight),
            StrokeKind::Inside,
        );
    }

    pub(super) fn drop(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        target: DropTarget,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let lower = response
            .hover_pos()
            .is_some_and(|pos| pos.y > response.rect.center().y);
        if response.dnd_hover_payload::<TabId>().is_some() {
            let y = if lower {
                response.rect.bottom()
            } else {
                response.rect.top()
            };
            ui.painter().line_segment(
                [
                    egui::pos2(response.rect.left(), y),
                    egui::pos2(response.rect.right(), y),
                ],
                Stroke::new(2.0, palette.text),
            );
        }
        if let Some(tab) = response.dnd_release_payload::<TabId>() {
            actions.push(Action::Command(Command::MoveTab {
                tab: *tab,
                to: drop_location(target, lower),
            }));
        }
    }

    pub(crate) fn menu(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        self.menu_at(ui, browser, palette, actions, None);
    }

    pub(crate) fn menu_at(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        palette: Palette,
        actions: &mut Vec<Action>,
        position: Option<egui::Pos2>,
    ) {
        let Some(menu) = self.menu else {
            return;
        };
        let pos = position.unwrap_or(match menu {
            Menu::Tab(_, pos) | Menu::Group(_, pos) => pos,
        });
        let sidebar = ui
            .ctx()
            .input(|input| input.viewport().inner_rect)
            .unwrap_or(ui.max_rect());
        let x = pos.x.min(sidebar.right() - 180.0).max(sidebar.left() + 4.0);
        let y = pos.y.min(sidebar.bottom() - 210.0).max(sidebar.top() + 4.0);
        let mut close = false;
        egui::Area::new(egui::Id::unique("sidebar_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x, y))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(palette.pane)
                    .show(ui, |ui| {
                        ui.set_min_width(170.0);
                        match menu {
                            Menu::Tab(id, _) => {
                                let pinned = browser
                                    .workspace(browser.active_workspace())
                                    .is_some_and(|ws| ws.pinned().contains(&id));
                                if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                                    actions.push(Action::Command(if pinned {
                                        Command::UnpinTab(id)
                                    } else {
                                        Command::PinTab(id)
                                    }));
                                    close = true;
                                }
                                if !pinned && ui.button("Add to new group").clicked() {
                                    actions.push(Action::Command(Command::CreateGroup {
                                        tabs: vec![id],
                                        name: "New Group".to_owned(),
                                        color: GroupColor::Blue,
                                    }));
                                    close = true;
                                }
                                if find_group(browser, id).is_some()
                                    && ui.button("Remove from group").clicked()
                                {
                                    actions.push(Action::Command(Command::MoveTab {
                                        tab: id,
                                        to: Location::Items(usize::MAX),
                                    }));
                                    close = true;
                                }
                                if ui.button("Close").clicked() {
                                    actions.push(Action::Command(Command::CloseTab(id)));
                                    close = true;
                                }
                            }
                            Menu::Group(id, _) => {
                                if ui.button("Rename").clicked() {
                                    let name = find_group_name(browser, id).unwrap_or_default();
                                    self.rename = Some((id, name));
                                    close = true;
                                }
                                ui.label(
                                    egui::RichText::new("Color")
                                        .size(11.0)
                                        .color(palette.secondary),
                                );
                                ui.horizontal(|ui| {
                                    for color in theme::GROUP_COLORS {
                                        let (rect, response) = ui.allocate_exact_size(
                                            egui::vec2(16.0, 16.0),
                                            Sense::click(),
                                        );
                                        ui.painter().circle_filled(
                                            rect.center(),
                                            6.0,
                                            theme::group_color(color),
                                        );
                                        response.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::Role::Button,
                                                true,
                                                format!("Group color {color:?}"),
                                            )
                                        });
                                        crate::icons::focus_ring(
                                            ui,
                                            &response,
                                            rect,
                                            self.theme.accent,
                                            8.0,
                                        );
                                        if response.clicked() {
                                            actions.push(Action::Command(Command::SetGroupColor {
                                                group: id,
                                                color,
                                            }));
                                            close = true;
                                        }
                                    }
                                });
                                if ui.button("Ungroup").clicked() {
                                    actions.push(Action::Command(Command::Ungroup(id)));
                                    close = true;
                                }
                                if ui.button("Close group").clicked() {
                                    actions.push(Action::Command(Command::CloseGroup(id)));
                                    close = true;
                                }
                            }
                        }
                    });
            });
        if close || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.menu = None;
        }
    }
}

fn find_group(browser: &Browser, tab: TabId) -> Option<GroupId> {
    let workspace = browser.workspace(browser.active_workspace())?;
    for item in workspace.items() {
        if let Item::Group(group) = item
            && group.tabs().contains(&tab)
        {
            return Some(group.id());
        }
    }
    None
}

fn find_group_name(browser: &Browser, id: GroupId) -> Option<String> {
    let workspace = browser.workspace(browser.active_workspace())?;
    for item in workspace.items() {
        if let Item::Group(group) = item
            && group.id() == id
        {
            return Some(group.name().to_owned());
        }
    }
    None
}

pub(crate) fn display_host(raw: &str) -> String {
    aegis_ipc::page_url(raw).map_or_else(
        |_| "New Tab".to_owned(),
        |url| aegis_ipc::display_host(&url),
    )
}

fn paint_chevron(ui: &egui::Ui, center: egui::Pos2, progress: f32, color: Color32) {
    if progress >= 0.999 {
        crate::icons::chevron_down(
            ui.painter(),
            egui::Rect::from_center_size(center, egui::vec2(16.0, 16.0)),
            color,
        );
        return;
    }
    let angle = progress * std::f32::consts::FRAC_PI_2;
    let (sin, cos) = angle.sin_cos();
    let rotate = |x: f32, y: f32| center + egui::vec2(x * cos - y * sin, x * sin + y * cos);
    let tip = rotate(2.0, 0.0);
    let stroke = Stroke::new(1.5, color);
    ui.painter().line_segment([rotate(-2.0, -4.0), tip], stroke);
    ui.painter().line_segment([tip, rotate(-2.0, 4.0)], stroke);
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

fn truncate_both(title: &str, host: &str, max_chars: usize) -> String {
    let mut result = String::with_capacity(max_chars);
    for ch in title
        .chars()
        .chain(" · ".chars())
        .chain(host.chars())
        .take(max_chars)
    {
        result.push(ch);
    }
    result
}
