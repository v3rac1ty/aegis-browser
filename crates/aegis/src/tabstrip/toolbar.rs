use aegis_core::Browser;
use eframe::egui::{self, Sense};

use super::TabStrip;
use crate::command_bar::Mode;
use crate::icons;
use crate::sidebar::{Action, Sidebar, display_host};
use crate::theme::Theme;

impl TabStrip {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        _sidebar: &mut Sidebar,
        loading: bool,
        blocked: u32,
        theme: Theme,
        actions: &mut Vec<Action>,
    ) -> egui::Rect {
        let row = ui.max_rect();
        ui.painter().rect_filled(row, 0.0, theme.palette.pane);
        if theme.pane_scrim.a() > 0 {
            ui.painter().rect_filled(row, 0.0, theme.pane_scrim);
        }
        let space = browser.workspace(browser.active_workspace());
        let switcher =
            egui::Rect::from_min_size(row.min + egui::vec2(8.0, 5.0), egui::vec2(112.0, 30.0));
        let switch = ui.interact(
            switcher,
            ui.scope_id().with("space_switcher"),
            Sense::click(),
        );
        icons::button_background(
            ui,
            &switch,
            switcher,
            theme.palette.hover,
            theme.palette.seg_on,
            theme.metrics.control_radius,
        );
        if let Some(space) = space {
            ui.painter().circle_filled(
                switcher.left_center() + egui::vec2(12.0, 0.0),
                4.0,
                crate::theme::group_color(space.color()),
            );
            ui.painter().with_clip_rect(switcher.shrink(4.0)).text(
                switcher.left_center() + egui::vec2(22.0, 0.0),
                egui::Align2::LEFT_CENTER,
                space.name(),
                crate::fonts::id(ui.ctx(), "Caption"),
                theme.palette.text,
            );
        }
        switch.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Switch space"));
        icons::focus_ring(
            ui,
            &switch,
            switcher,
            theme.accent,
            theme.metrics.control_radius,
        );
        if switch.clicked() {
            self.space_open = Some(switcher.left_bottom());
        }
        let back =
            egui::Rect::from_min_size(row.min + egui::vec2(124.0, 5.0), egui::vec2(30.0, 30.0));
        let forward = back.translate(egui::vec2(32.0, 0.0));
        let reload = forward.translate(egui::vec2(32.0, 0.0));
        let more = egui::Rect::from_min_size(
            egui::pos2(row.right() - 38.0, row.top() + 5.0),
            egui::vec2(30.0, 30.0),
        );
        for (rect, icon, label, action) in [
            (
                back,
                icons::chevron_left as fn(&egui::Painter, egui::Rect, egui::Color32),
                "Back",
                Action::Back,
            ),
            (forward, icons::chevron_right, "Forward", Action::Forward),
            (
                reload,
                if loading { icons::close } else { icons::reload },
                if loading { "Stop" } else { "Reload" },
                if loading {
                    Action::Stop
                } else {
                    Action::Reload
                },
            ),
        ] {
            let response = ui.interact(rect, ui.scope_id().with(label), Sense::click());
            icons::button_background(
                ui,
                &response,
                rect,
                theme.palette.hover,
                theme.palette.seg_on,
                theme.metrics.control_radius,
            );
            icon(ui.painter(), rect.shrink(6.0), theme.palette.secondary);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            icons::focus_ring(
                ui,
                &response,
                rect,
                theme.accent,
                theme.metrics.control_radius,
            );
            if response.clicked() {
                actions.push(action);
            }
        }
        let menu = ui.interact(more, ui.scope_id().with("settings"), Sense::click());
        icons::button_background(
            ui,
            &menu,
            more,
            theme.palette.hover,
            theme.palette.seg_on,
            theme.metrics.control_radius,
        );
        icons::ellipsis(ui.painter(), more.shrink(6.0), theme.palette.secondary);
        menu.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Settings"));
        icons::focus_ring(ui, &menu, more, theme.accent, theme.metrics.control_radius);
        if menu.clicked() {
            actions.push(Action::OpenSettings);
        }

        let width = 560.0_f32.min((row.width() - 240.0).max(100.0));
        let capsule = egui::Rect::from_center_size(
            egui::pos2(row.center().x, row.center().y),
            egui::vec2(width, 32.0),
        );
        ui.painter()
            .rect_filled(capsule, 14.0, theme.palette.capsule);
        let count = crate::protections::tracker_blocking_active() && cfg!(target_os = "windows");
        let shield = egui::Rect::from_min_size(
            capsule.min + egui::vec2(4.0, 1.0),
            egui::vec2(if count { 42.0 } else { 30.0 }, 30.0),
        );
        let status = ui.interact(shield, ui.scope_id().with("privacy"), Sense::click());
        let shield_icon =
            egui::Rect::from_min_size(shield.min + egui::vec2(3.0, 5.0), egui::vec2(20.0, 20.0));
        if crate::protections::tracker_blocking_active() {
            icons::shield_check(ui.painter(), shield_icon, theme.accent);
        } else {
            icons::shield(ui.painter(), shield_icon, theme.accent);
        }
        if count {
            ui.painter().text(
                shield.right_center() - egui::vec2(4.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                blocked.to_string(),
                crate::fonts::id(ui.ctx(), "Caption"),
                theme.palette.text,
            );
        }
        status
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Privacy status"));
        icons::focus_ring(ui, &status, shield, theme.accent, 8.0);
        if status.clicked() {
            self.privacy_open = !self.privacy_open;
            self.privacy_anchor = Some(shield.left_bottom());
        }
        let host = browser
            .active_tab()
            .and_then(|id| browser.tab(id))
            .map_or_else(|| "New Tab".to_owned(), |tab| display_host(tab.url()));
        let mut address_right = capsule.right() - 4.0;
        if crate::passwords::has_logins() {
            let key_rect = egui::Rect::from_min_size(
                egui::pos2(capsule.right() - 34.0, capsule.top() + 1.0),
                egui::vec2(30.0, 30.0),
            );
            let key = ui.interact(key_rect, ui.scope_id().with("passwords"), Sense::click());
            icons::button_background(
                ui,
                &key,
                key_rect,
                theme.palette.hover,
                theme.palette.seg_on,
                theme.metrics.control_radius,
            );
            icons::lock(ui.painter(), key_rect.shrink(7.0), theme.palette.text);
            key.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Saved logins"));
            if key.clicked() {
                actions.push(Action::OpenPasswords);
            }
            address_right = key_rect.left();
        }
        let address = egui::Rect::from_min_max(
            egui::pos2(shield.right(), capsule.top()),
            egui::pos2(address_right, capsule.bottom()),
        );
        let response = ui.interact(address, ui.scope_id().with("address"), Sense::click());
        ui.painter().with_clip_rect(address).text(
            capsule.center(),
            egui::Align2::CENTER_CENTER,
            &host,
            crate::fonts::id(ui.ctx(), "Caption"),
            theme.palette.text,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Address: {host}"))
        });
        icons::focus_ring(ui, &response, address, theme.accent, 8.0);
        if response.clicked() {
            actions.push(Action::OpenBar(Mode::CurrentTab));
        }
        more
    }
}
