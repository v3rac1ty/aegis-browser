use super::*;
use crate::icons;

impl Sidebar {
    pub(super) fn title_row(
        &mut self,
        ui: &mut egui::Ui,
        compact: bool,
        actions: &mut Vec<Action>,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 28.0),
            Sense::click_and_drag(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Move window"));
        #[cfg(target_os = "macos")]
        let drag_rect = if self.right {
            egui::Rect::from_min_max(
                rect.min + egui::vec2(30.0, 0.0),
                rect.max - egui::vec2(72.0, 0.0),
            )
        } else {
            egui::Rect::from_min_max(
                rect.min + egui::vec2(72.0, 0.0),
                rect.max - egui::vec2(30.0, 0.0),
            )
        };
        #[cfg(not(target_os = "macos"))]
        let drag_rect = if self.right {
            egui::Rect::from_min_max(rect.min + egui::vec2(30.0, 0.0), rect.max)
        } else {
            egui::Rect::from_min_max(rect.min, rect.max - egui::vec2(30.0, 0.0))
        };
        crate::widgets::window_drag(ui, &response, drag_rect);
        let toggle_rect = egui::Rect::from_min_size(
            if self.right {
                rect.left_top()
            } else {
                rect.right_top() - egui::vec2(28.0, 0.0)
            },
            egui::vec2(28.0, 28.0),
        );
        let response = ui.put(toggle_rect, egui::Button::new(" ").frame(false));
        icons::button_background(
            ui,
            &response,
            toggle_rect,
            self.theme.palette.hover,
            self.theme.palette.seg_on,
            8.0,
        );
        icons::sidebar_toggle(
            ui.painter(),
            toggle_rect.shrink(5.0),
            self.theme.palette.secondary,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, "Toggle compact sidebar")
        });
        icons::focus_ring(ui, &response, toggle_rect, self.theme.accent, 8.0);
        if response.clicked() {
            actions.push(Action::SetCompact(!compact));
        }
    }

    pub(super) fn capsule(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        loading: bool,
        blocked: u32,
        actions: &mut Vec<Action>,
    ) -> egui::Rect {
        let palette = self.theme.palette;
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), Sense::hover());
        ui.painter().rect_filled(rect, 14.0, palette.capsule);
        let mut x = if self.right {
            rect.right() - 4.0
        } else {
            rect.left() + 4.0
        };
        let icon_size = 30.0;
        let active_tab = browser.active_tab().and_then(|id| browser.tab(id));
        let back_enabled =
            active_tab.is_some_and(|tab| tab.history().len() > 1 || tab.opener().is_some());
        let view_enabled = active_tab.is_some();
        for (icon, label, action) in [
            (icons::chevron_left as icons::Icon, "Back", Action::Back),
            (
                icons::chevron_right as icons::Icon,
                "Forward",
                Action::Forward,
            ),
        ] {
            let button = egui::Rect::from_min_size(
                egui::pos2(if self.right { x - icon_size } else { x }, rect.top() + 4.0),
                egui::vec2(icon_size, icon_size),
            );
            let enabled = if matches!(&action, Action::Back) {
                back_enabled
            } else {
                view_enabled
            };
            let response = ui.interact(
                button,
                ui.scope_id().with(label),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            if enabled {
                icons::button_background(ui, &response, button, palette.hover, palette.seg_on, 8.0);
            }
            icon(
                ui.painter(),
                button.shrink(5.0),
                if enabled {
                    palette.text
                } else {
                    palette.tertiary
                },
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
            icons::focus_ring(ui, &response, button, self.theme.accent, 8.0);
            if response.clicked() {
                actions.push(action);
            }
            x += if self.right { -icon_size } else { icon_size };
        }
        let reload_rect = egui::Rect::from_min_size(
            egui::pos2(if self.right { x - icon_size } else { x }, rect.top() + 4.0),
            egui::vec2(icon_size, icon_size),
        );
        let reload = ui.interact(
            reload_rect,
            ui.scope_id().with("reload"),
            if view_enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if view_enabled {
            icons::button_background(ui, &reload, reload_rect, palette.hover, palette.seg_on, 8.0);
        }
        if loading {
            icons::close(
                ui.painter(),
                reload_rect.shrink(6.0),
                if view_enabled {
                    palette.secondary
                } else {
                    palette.tertiary
                },
            );
        } else {
            icons::reload(
                ui.painter(),
                reload_rect.shrink(6.0),
                if view_enabled {
                    palette.secondary
                } else {
                    palette.tertiary
                },
            );
        }
        reload.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                view_enabled,
                if loading { "Stop loading" } else { "Reload" },
            )
        });
        icons::focus_ring(ui, &reload, reload_rect, self.theme.accent, 8.0);
        if reload.clicked() {
            actions.push(if loading {
                Action::Stop
            } else {
                Action::Reload
            });
        }
        x += if self.right { -icon_size } else { icon_size };
        let show_count = blocked > 0
            && crate::protections::tracker_blocking_active()
            && cfg!(target_os = "windows");
        let shield_width = if show_count { 42.0 } else { 30.0 };
        let shield_rect = egui::Rect::from_min_size(
            egui::pos2(
                if self.right { x - shield_width } else { x },
                rect.top() + 4.0,
            ),
            egui::vec2(shield_width, icon_size),
        );
        let shield = ui.interact(shield_rect, ui.scope_id().with("privacy"), Sense::click());
        icons::button_background(ui, &shield, shield_rect, palette.hover, palette.seg_on, 8.0);
        let shield_icon = egui::Rect::from_min_size(
            shield_rect.min + egui::vec2(3.0, 5.0),
            egui::vec2(20.0, 20.0),
        );
        if crate::protections::tracker_blocking_active() {
            icons::shield_check(ui.painter(), shield_icon, self.theme.accent);
        } else {
            icons::shield(ui.painter(), shield_icon, self.theme.accent);
        }
        if show_count {
            ui.painter().text(
                shield_rect.right_center() - egui::vec2(4.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                blocked.to_string(),
                crate::fonts::id(ui.ctx(), "Caption"),
                palette.text,
            );
        }
        shield
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Privacy status"));
        icons::focus_ring(ui, &shield, shield_rect, self.theme.accent, 8.0);
        if shield.clicked() {
            self.privacy_open = !self.privacy_open;
        }
        x = if self.right {
            shield_rect.left()
        } else {
            shield_rect.right()
        };
        if crate::passwords::has_logins() {
            let key_rect = egui::Rect::from_min_size(
                egui::pos2(if self.right { x - 30.0 } else { x }, rect.top() + 4.0),
                egui::vec2(30.0, 30.0),
            );
            let key = ui.interact(key_rect, ui.scope_id().with("passwords"), Sense::click());
            icons::button_background(ui, &key, key_rect, palette.hover, palette.seg_on, 8.0);
            icons::lock(ui.painter(), key_rect.shrink(7.0), palette.text);
            key.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Saved logins"));
            if key.clicked() {
                actions.push(Action::OpenPasswords);
            }
            x += if self.right { -30.0 } else { 30.0 };
        }
        let menu_rect = egui::Rect::from_min_size(
            if self.right {
                rect.left_top() + egui::vec2(4.0, 4.0)
            } else {
                rect.right_top() + egui::vec2(-34.0, 4.0)
            },
            egui::vec2(30.0, 30.0),
        );
        let menu = ui.interact(menu_rect, ui.scope_id().with("more"), Sense::click());
        icons::button_background(ui, &menu, menu_rect, palette.hover, palette.seg_on, 8.0);
        icons::ellipsis(ui.painter(), menu_rect.shrink(5.0), palette.secondary);
        menu.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Settings"));
        icons::focus_ring(ui, &menu, menu_rect, self.theme.accent, 8.0);
        if menu.clicked() {
            actions.push(Action::OpenSettings);
        }
        let host = browser
            .active_tab()
            .and_then(|id| browser.tab(id))
            .map_or_else(|| "New Tab".to_owned(), |tab| display_host(tab.url()));
        let host_rect = if self.right {
            egui::Rect::from_min_max(
                egui::pos2(menu_rect.right(), rect.top() + 4.0),
                egui::pos2(x, rect.bottom() - 4.0),
            )
        } else {
            egui::Rect::from_min_max(
                egui::pos2(x, rect.top() + 4.0),
                egui::pos2(menu_rect.left(), rect.bottom() - 4.0),
            )
        };
        let host_response = ui.interact(host_rect, ui.scope_id().with("address"), Sense::click());
        icons::button_background(
            ui,
            &host_response,
            host_rect,
            palette.hover,
            palette.seg_on,
            8.0,
        );
        host_response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Address: {host}"))
        });
        // the text only truncates when it really does not fit
        let mut job = egui::text::LayoutJob::simple_singleline(
            host,
            crate::fonts::id(ui.ctx(), "Caption"),
            palette.text,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(host_text_width(host_rect.width()));
        let galley = ui.painter().layout_job(job);
        let text_pos = if self.right {
            egui::pos2(
                host_rect.right() - HOST_INSET - galley.size().x,
                host_rect.center().y - galley.size().y / 2.0,
            )
        } else {
            egui::pos2(
                host_rect.left() + HOST_INSET,
                host_rect.center().y - galley.size().y / 2.0,
            )
        };
        ui.painter().galley(text_pos, galley, palette.text);
        icons::focus_ring(ui, &host_response, host_rect, self.theme.accent, 8.0);
        if host_response.clicked() {
            actions.push(Action::OpenBar(Mode::CurrentTab));
        }
        if self.privacy_open {
            self.privacy(ui, browser, blocked, rect);
        }
        menu_rect
    }

    fn privacy(&mut self, ui: &egui::Ui, browser: &Browser, blocked: u32, capsule: egui::Rect) {
        let host = browser
            .active_tab()
            .and_then(|id| browser.tab(id))
            .map_or_else(|| "this page".to_owned(), |tab| display_host(tab.url()));
        let pos = capsule.left_top() - egui::vec2(0.0, 212.0);
        egui::Area::new(egui::Id::unique("privacy_popover"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(self.theme.palette.overlay)
                    .corner_radius(self.theme.metrics.row_radius + 2.0)
                    .show(ui, |ui| {
                        ui.set_min_width(220.0);
                        crate::protections::show(ui, self.theme, &host, blocked);
                    });
            });
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.privacy_open = false;
        }
    }
}

// space between the host text and the shield side of its button
const HOST_INSET: f32 = 4.0;
// gap between the host text and the menu button
const HOST_GAP: f32 = 6.0;

// width the host text may use inside the button between the shield and the menu
fn host_text_width(button_width: f32) -> f32 {
    (button_width - HOST_INSET - HOST_GAP).max(0.0)
}

#[cfg(test)]
mod host_tests {
    use super::{HOST_GAP, HOST_INSET, host_text_width};
    use eframe::egui;

    #[test]
    fn host_width_uses_all_room_but_the_gap() {
        assert_eq!(host_text_width(100.0), 100.0 - HOST_INSET - HOST_GAP);
        assert_eq!(host_text_width(3.0), 0.0);
    }

    #[test]
    fn example_host_fits_at_default_sidebar_width() {
        // default width 272, back/forward/reload 90, shield 30, key 30, menu 34, edge 4
        let button = 272.0 - 4.0 - 90.0 - 30.0 - 30.0 - 34.0;
        let ctx = egui::Context::default();
        let mut width = f32::MAX;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let galley = ui.painter().layout_no_wrap(
                "example.com".to_owned(),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
            width = galley.size().x;
        });
        output.textures_delta.clear();
        assert!(width <= host_text_width(button), "{width} > {button}");
    }
}
