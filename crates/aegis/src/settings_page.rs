use aegis_core::{
    Accent, Command, Corners, Density, FrameLimit, Glass, Layout, MotionPref, PinnedStyle,
    Settings, Setup, Side, ThemeMode,
};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, StrokeKind};

use crate::theme::{self, Theme};
use crate::{accent_picker, imports, passwords};

mod sidebar;
pub(crate) mod ui;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Page {
    #[default]
    Appearance,
    Sidebar,
    Privacy,
    Passwords,
    Import,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Self::Appearance,
        Self::Sidebar,
        Self::Privacy,
        Self::Passwords,
        Self::Import,
    ];

    fn index(self) -> usize {
        match self {
            Self::Appearance => 0,
            Self::Sidebar => 1,
            Self::Privacy => 2,
            Self::Passwords => 3,
            Self::Import => 4,
        }
    }

    pub fn from_debug(value: &str) -> Option<Self> {
        match value {
            "appearance" => Some(Self::Appearance),
            "sidebar" => Some(Self::Sidebar),
            "privacy" => Some(Self::Privacy),
            "passwords" => Some(Self::Passwords),
            "import" => Some(Self::Import),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Sidebar => "Sidebar",
            Self::Privacy => "Privacy and security",
            Self::Passwords => "Passwords",
            Self::Import => "Import",
        }
    }

    fn color(self, theme: Theme) -> Color32 {
        match self {
            Self::Appearance => theme.accent,
            Self::Sidebar => Color32::from_rgb(12, 133, 153),
            Self::Privacy => Color32::from_rgb(43, 138, 62),
            Self::Passwords => Color32::from_rgb(112, 72, 232),
            Self::Import => Color32::from_rgb(217, 72, 15),
        }
    }

    fn icon(self) -> crate::icons::Icon {
        match self {
            Self::Appearance => crate::icons::gear,
            Self::Sidebar => crate::icons::sidebar_toggle,
            Self::Privacy => crate::icons::shield_check,
            Self::Passwords => crate::icons::lock,
            Self::Import => crate::icons::reload,
        }
    }
}

#[derive(Default)]
pub struct SettingsPage {
    pub selected: Page,
    accent_picker: crate::accent_picker::AccentPicker,
    setup_motion: crate::motion::Motion,
    nav_motion: crate::motion::Motion,
    section_drag: Option<sidebar::SectionDrag>,
    #[cfg_attr(not(test), allow(dead_code))]
    sidebar_laid: sidebar::Laid,
}

impl SettingsPage {
    pub fn with_page(selected: Page) -> Self {
        Self {
            selected,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Change {
    Layout(Layout),
    SidebarSide(Side),
    FloatingPage(bool),
    Theme(ThemeMode),
    Glass(Glass),
    Noise(u8),
    SidebarOpacity(u8),
    Corners(Corners),
    PinnedStyle(PinnedStyle),
    Density(Density),
    TextScale(u16),
    Motion(MotionPref),
    FrameLimit(FrameLimit),
    CustomFps(u16),
    SmoothScrolling(bool),
    #[cfg(not(target_os = "macos"))]
    HardwareAcceleration(bool),
}

pub fn apply(settings: &mut Settings, change: Change) {
    match change {
        Change::Layout(value) => {
            settings.layout = value;
            settings.setup = Setup::Custom;
        }
        Change::SidebarSide(value) => {
            settings.sidebar_side = value;
            settings.setup = Setup::Custom;
        }
        Change::FloatingPage(value) => settings.floating_page = value,
        Change::Theme(value) => settings.theme = value,
        Change::Glass(value) => settings.glass = value,
        Change::Noise(value) => settings.noise = value,
        Change::SidebarOpacity(value) => settings.sidebar_opacity = value,
        Change::Corners(value) => settings.corners = value,
        Change::PinnedStyle(value) => {
            settings.pinned_style = value;
            settings.setup = Setup::Custom;
        }
        Change::Density(value) => {
            settings.density = value;
            settings.setup = Setup::Custom;
        }
        Change::TextScale(value) => settings.text_scale = value,
        Change::Motion(value) => settings.motion = value,
        Change::FrameLimit(value) => settings.frame_limit = value,
        Change::CustomFps(value) => settings.custom_fps = value,
        Change::SmoothScrolling(value) => settings.smooth_scrolling = value,
        #[cfg(not(target_os = "macos"))]
        Change::HardwareAcceleration(value) => settings.hardware_acceleration = value,
    }
}

pub(super) fn row(
    ui: &mut egui::Ui,
    label: &str,
    theme: Theme,
    options: &[&str],
    selected: usize,
) -> Option<usize> {
    ui::row(ui, theme, label, ui::SEGMENTED, |ui| {
        ui::segmented(ui, label, options, selected, theme)
    })
}

pub(super) fn card(
    ui: &mut egui::Ui,
    theme: Theme,
    title: &str,
    add: impl FnOnce(&mut egui::Ui),
) -> Rect {
    ui::section(ui, theme, title, add)
}

const ACCENT_SWATCH: f32 = 28.0;
const NAV_ROW_HEIGHT: f32 = 32.0;
const NAV_GAP: f32 = 2.0;
// header: 12 above the back button, 28 for it, 16 below
const NAV_HEADER: f32 = 56.0;

// distance between the tops of two nav rows
const NAV_PITCH: f32 = NAV_ROW_HEIGHT + NAV_GAP;

fn nav_row_size() -> egui::Vec2 {
    egui::vec2(204.0, NAV_ROW_HEIGHT)
}

// rows and the selection fill both use this, so they cannot drift apart
fn nav_row_top(list_top: f32, index: usize) -> f32 {
    list_top + index as f32 * NAV_PITCH
}

type ShowResult = (
    Option<Command>,
    bool,
    bool,
    bool,
    Vec<passwords::Action>,
    Vec<imports::Action>,
);

pub fn show(
    ui: &mut egui::Ui,
    window: Rect,
    settings: &Settings,
    theme: Theme,
    state: &mut SettingsPage,
    passwords: &mut passwords::Passwords,
    imports: &mut imports::Imports,
) -> ShowResult {
    let panes = crate::app::pane_layout_at_scale(
        window,
        220.0,
        settings.sidebar_side,
        ui.ctx().pixels_per_point(),
    );
    let mut next = settings.clone();
    let mut close = false;
    let mut clear_exceptions = false;
    let mut clear_icons = false;
    let mut sidebar_command = None;
    let mut password_actions = Vec::new();
    let mut import_actions = Vec::new();
    egui::Area::new(egui::Id::unique("settings_nav"))
        .fixed_pos(panes.sidebar_rect.min)
        .show(ui.ctx(), |ui| {
            let rect = Rect::from_min_size(ui.min_rect().min, panes.sidebar_rect.size());
            ui.set_min_size(rect.size());
            ui.set_clip_rect(rect);
            ui.painter()
                .rect_filled(rect, theme.metrics.pane_radius, theme.palette.pane);
            if theme.pane_scrim.a() > 0 {
                ui.painter()
                    .rect_filled(rect, theme.metrics.pane_radius, theme.pane_scrim);
            }
            ui.painter().rect_stroke(
                rect,
                theme.metrics.pane_radius,
                Stroke::new(theme::hairline(ui.ctx()), theme.palette.pane_edge),
                StrokeKind::Inside,
            );
            // header: back button and title share one click target
            let back =
                Rect::from_min_size(rect.min + egui::vec2(8.0, 12.0), egui::vec2(28.0, 28.0));
            let back_response =
                ui.interact(back, ui.scope_id().with("settings_back"), Sense::click());
            crate::icons::button_background(
                ui,
                &back_response,
                back,
                theme.palette.hover,
                theme.palette.seg_on,
                7.0,
            );
            crate::icons::chevron_left(ui.painter(), back.shrink(5.0), theme.palette.text);
            back_response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, "Close settings")
            });
            crate::icons::focus_ring(ui, &back_response, back, theme.accent, 7.0);
            let title = Rect::from_min_size(
                egui::pos2(back.right(), back.top()),
                egui::vec2(130.0, 28.0),
            );
            let title_response =
                ui.interact(title, ui.scope_id().with("settings_title"), Sense::click());
            ui.painter().text(
                title.left_center(),
                egui::Align2::LEFT_CENTER,
                "Settings",
                crate::fonts::id(ui.ctx(), "Heading"),
                theme.palette.text,
            );
            title_response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, "Close settings")
            });
            if back_response.clicked() || title_response.clicked() {
                close = true;
            }
            // rows and the selection fill both come from nav_row_top
            let list_top = rect.top() + NAV_HEADER;
            let dt = state.nav_motion.frame();
            let y = state.nav_motion.value(
                ui.ctx(),
                ui.scope_id().with("nav_selection"),
                nav_row_top(list_top, state.selected.index()),
                0.20,
                dt,
                settings.motion,
            );
            let selected_rect =
                Rect::from_min_size(egui::pos2(rect.left() + 8.0, y), nav_row_size());
            if !theme.dark {
                ui.painter().add(
                    egui::Shadow {
                        offset: [0, 1],
                        blur: 3,
                        spread: 0,
                        color: Color32::from_black_alpha(26),
                    }
                    .as_shape(selected_rect, 9.0),
                );
            }
            ui.painter()
                .rect_filled(selected_rect, 9.0, theme.palette.row_on);
            for page in Page::ALL {
                let row = Rect::from_min_size(
                    egui::pos2(rect.left() + 8.0, nav_row_top(list_top, page.index())),
                    nav_row_size(),
                );
                let response = ui.interact(
                    row,
                    ui.scope_id().with(("settings_nav_row", page.index())),
                    Sense::click(),
                );
                let hover = ui.ctx().animate_bool_with_time(
                    response.id.with("hover"),
                    response.hovered(),
                    0.12,
                );
                if state.selected != page && hover > 0.0 {
                    ui.painter()
                        .rect_filled(row, 9.0, theme.palette.hover.gamma_multiply(hover));
                }
                let square =
                    Rect::from_min_size(row.min + egui::vec2(10.0, 7.0), egui::vec2(18.0, 18.0));
                ui.painter().rect_filled(square, 6.0, page.color(theme));
                page.icon()(ui.painter(), square.shrink(3.0), Color32::WHITE);
                let font = if state.selected == page {
                    crate::fonts::semibold(13.0)
                } else {
                    egui::FontId::proportional(13.0)
                };
                ui.painter().text(
                    row.left_center() + egui::vec2(38.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    page.title(),
                    font,
                    theme.palette.text,
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, page.title())
                });
                if response.clicked() {
                    state.selected = page;
                }
            }
        });
    egui::Area::new(egui::Id::unique("settings_content"))
        .fixed_pos(panes.page_rect.min)
        .show(ui.ctx(), |ui| {
            let rect = Rect::from_min_size(ui.min_rect().min, panes.page_rect.size());
            ui.set_min_size(rect.size());
            ui.set_clip_rect(rect);
            ui.painter().rect_filled(rect, 13.0, theme.palette.content);
            ui.painter().rect_stroke(
                rect,
                13.0,
                Stroke::new(theme::hairline(ui.ctx()), theme.palette.card_edge),
                StrokeKind::Inside,
            );
            // the whole column scrolls as one, wide sidebar page included
            egui::ScrollArea::vertical()
                .id_salt("settings_page_scroll")
                .auto_shrink([false, false])
                .max_height(rect.height())
                .show(ui, |ui| {
                    let width = if state.selected == Page::Sidebar && rect.width() >= 1000.0 {
                        (rect.width() - 80.0).clamp(0.0, 1040.0)
                    } else {
                        (rect.width() - 80.0).clamp(0.0, 640.0)
                    };
                    ui.add_space(32.0);
                    ui.horizontal(|ui| {
                        ui.add_space(40.0_f32.min(rect.width() * 0.1));
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(
                                egui::RichText::new(state.selected.title())
                                    .font(crate::fonts::semibold(24.0))
                                    .color(theme.palette.text),
                            );
                            ui.add_space(26.0);
                            match state.selected {
                                Page::Appearance => {
                                    appearance(ui, &mut next, theme, &mut state.accent_picker)
                                }
                                Page::Sidebar => {
                                    sidebar_command = sidebar::show(
                                        ui,
                                        &next,
                                        theme,
                                        &mut state.setup_motion,
                                        &mut state.section_drag,
                                        &mut state.sidebar_laid,
                                        rect.width(),
                                        rect.height(),
                                    )
                                }
                                Page::Privacy => privacy(
                                    ui,
                                    &mut next,
                                    theme,
                                    passwords,
                                    &mut clear_exceptions,
                                    &mut clear_icons,
                                ),
                                Page::Passwords => {
                                    password_actions = passwords.show_settings(ui, theme)
                                }
                                Page::Import => import_actions = imports.show(ui, theme),
                            }
                            ui.add_space(48.0);
                        });
                    });
                });
        });
    (
        sidebar_command.or_else(|| (next != *settings).then_some(Command::SetSettings(next))),
        close,
        clear_exceptions,
        clear_icons,
        password_actions,
        import_actions,
    )
}

fn appearance(
    ui: &mut egui::Ui,
    next: &mut Settings,
    theme: Theme,
    accent_picker: &mut crate::accent_picker::AccentPicker,
) {
    card(ui, theme, "LAYOUT", |ui| {
        if let Some(i) = row(
            ui,
            "Tabs",
            theme,
            &["Sidebar", "Classic tabs"],
            usize::from(next.layout == Layout::Horizontal),
        ) {
            apply(next, Change::Layout(Layout::ALL[i]));
        }
        ui::hairline(ui, theme);
        if let Some(i) = row(
            ui,
            "Sidebar position",
            theme,
            &["Left", "Right"],
            usize::from(next.sidebar_side == Side::Right),
        ) {
            apply(next, Change::SidebarSide(Side::ALL[i]));
        }
        ui::hairline(ui, theme);
        let mut floating = next.floating_page;
        ui::row_hint(
            ui,
            theme,
            "Floating page",
            Some("Inset the page as a rounded pane over the glass"),
            ui::SWITCH,
            |ui| {
                ui::switch(ui, &mut floating, "Floating page", theme);
            },
        );
        if floating != next.floating_page {
            apply(next, Change::FloatingPage(floating));
        }
    });
    card(ui, theme, "LOOK", |ui| {
        if let Some(i) = row(
            ui,
            "Theme",
            theme,
            &["System", "Light", "Dark"],
            ThemeMode::ALL
                .iter()
                .position(|v| *v == next.theme)
                .unwrap_or(0),
        ) {
            apply(next, Change::Theme(ThemeMode::ALL[i]));
        }
        ui::hairline(ui, theme);
        let swatches = ui::Size {
            w: accent_picker::strip_width(ACCENT_SWATCH, Accent::ALL.len() + 1),
            h: ACCENT_SWATCH,
        };
        ui::row(ui, theme, "Accent", swatches, |ui| {
            accent_picker.swatches(
                ui,
                next,
                ACCENT_SWATCH,
                theme.palette.card,
                theme.palette.text,
            );
        });
        if accent_picker.open {
            ui::hairline(ui, theme);
            ui::block(ui, |ui| accent_picker.wheel(ui, next, next.motion));
        }
        ui::hairline(ui, theme);
        if let Some(i) = ui::row_hint(
            ui,
            theme,
            "Window glass",
            Some("Tinted follows the current space's color"),
            ui::SEGMENTED,
            |ui| {
                ui::segmented(
                    ui,
                    "Window glass",
                    &["Clear", "Tinted", "Solid"],
                    Glass::ALL
                        .iter()
                        .position(|v| *v == next.glass)
                        .unwrap_or(0),
                    theme,
                )
            },
        ) {
            apply(next, Change::Glass(Glass::ALL[i]));
        }
        ui::hairline(ui, theme);
        ui::row_hint(
            ui,
            theme,
            "Noise",
            Some("Film grain over the glass gradient"),
            ui::SLIDER,
            |ui| {
                let mut value = next.noise as u16;
                if ui::slider(ui, &mut value, 0..=100, 5, "%", theme) {
                    next.noise = value as u8;
                }
            },
        );
        ui::hairline(ui, theme);
        ui::row(ui, theme, "Sidebar opacity", ui::SLIDER, |ui| {
            let mut value = next.sidebar_opacity as u16;
            if ui::slider(ui, &mut value, 30..=100, 5, "%", theme) {
                next.sidebar_opacity = value as u8;
            }
        });
        ui::hairline(ui, theme);
        if let Some(i) = row(
            ui,
            "Corners",
            theme,
            &["Round", "Soft", "Square"],
            Corners::ALL
                .iter()
                .position(|v| *v == next.corners)
                .unwrap_or(0),
        ) {
            apply(next, Change::Corners(Corners::ALL[i]));
        }
        ui::hairline(ui, theme);
        if let Some(i) = row(
            ui,
            "Pinned sites",
            theme,
            &["Circles", "Tiles"],
            usize::from(next.pinned_style == PinnedStyle::Tiles),
        ) {
            apply(next, Change::PinnedStyle(PinnedStyle::ALL[i]));
        }
    });
    card(ui, theme, "COMFORT", |ui| {
        if let Some(i) = row(
            ui,
            "Density",
            theme,
            &["Compact", "Comfortable", "Spacious"],
            Density::ALL
                .iter()
                .position(|v| *v == next.density)
                .unwrap_or(1),
        ) {
            apply(next, Change::Density(Density::ALL[i]));
        }
        ui::hairline(ui, theme);
        let mut text_scale = next.text_scale;
        ui::row(ui, theme, "Interface text size", ui::SLIDER, |ui| {
            ui::slider(ui, &mut text_scale, 85..=130, 5, "%", theme);
        });
        if text_scale != next.text_scale {
            apply(next, Change::TextScale(text_scale));
        }
        ui::hairline(ui, theme);
        if let Some(i) = row(
            ui,
            "Motion",
            theme,
            &["System", "Full", "Reduced"],
            MotionPref::ALL
                .iter()
                .position(|v| *v == next.motion)
                .unwrap_or(0),
        ) {
            apply(next, Change::Motion(MotionPref::ALL[i]));
        }
        ui::hairline(ui, theme);
        if let Some(i) = row(
            ui,
            "Frame rate",
            theme,
            &["Match display", "VSync", "Custom", "Uncapped"],
            FrameLimit::ALL
                .iter()
                .position(|v| *v == next.frame_limit)
                .unwrap_or(0),
        ) {
            apply(next, Change::FrameLimit(FrameLimit::ALL[i]));
        }
        if next.frame_limit == FrameLimit::Custom {
            ui::hairline(ui, theme);
            let mut fps = next.custom_fps;
            ui::row(ui, theme, "Custom frame rate", ui::SLIDER, |ui| {
                ui::slider(ui, &mut fps, 30..=500, 10, " fps", theme);
            });
            if fps != next.custom_fps {
                apply(next, Change::CustomFps(fps));
            }
        }
        if next.frame_limit == FrameLimit::Vsync {
            ui::hairline(ui, theme);
            ui::row_hint(
                ui,
                theme,
                "Frame rate",
                Some("Applies after you restart Aegis"),
                ui::NONE,
                |_| {},
            );
        }
        ui::hairline(ui, theme);
        let mut smooth = next.smooth_scrolling;
        ui::row_hint(
            ui,
            theme,
            "Smooth scrolling",
            Some(smooth_scrolling_hint()),
            ui::SWITCH,
            |ui| {
                ui::switch(ui, &mut smooth, "Smooth scrolling", theme);
            },
        );
        if smooth != next.smooth_scrolling {
            apply(next, Change::SmoothScrolling(smooth));
        }
        // wkwebview has no public hardware acceleration setting
        #[cfg(not(target_os = "macos"))]
        {
            ui::hairline(ui, theme);
            let mut hardware = next.hardware_acceleration;
            ui::row_hint(
                ui,
                theme,
                "Hardware acceleration",
                Some(hardware_acceleration_hint()),
                ui::SWITCH,
                |ui| ui::switch(ui, &mut hardware, "Hardware acceleration", theme),
            );
            if hardware != next.hardware_acceleration {
                apply(next, Change::HardwareAcceleration(hardware));
            }
        }
    });
}

// pages follow the setting at launch on windows and live on linux
fn smooth_scrolling_hint() -> &'static str {
    if cfg!(target_os = "windows") {
        "Pages update after you restart Aegis"
    } else if cfg!(target_os = "linux") {
        "Applies to pages and Aegis"
    } else {
        "Applies to Aegis. Pages follow your system"
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn hardware_acceleration_hint() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Pages update after you restart Aegis. Turning this off uses more CPU"
    }
    #[cfg(target_os = "linux")]
    {
        "Pages update after you restart Aegis. Turning this off uses more CPU"
    }
}

fn privacy(
    ui: &mut egui::Ui,
    next: &mut Settings,
    theme: Theme,
    passwords: &passwords::Passwords,
    clear_exceptions: &mut bool,
    clear_icons: &mut bool,
) {
    card(ui, theme, "PROTECTIONS", |ui| {
        let protections = crate::protections::protections(crate::protections::ProtectionState {
            lists_loaded: crate::protections::state().lists_loaded,
            block_trackers: next.block_trackers,
            https_only: next.https_only,
            vault_exists: passwords.state.exists(),
            vault_os_protected: passwords.state.os_protected(),
        });
        for (index, protection) in protections.iter().enumerate() {
            ui::protection_row(
                ui,
                theme,
                protection.title,
                protection.detail,
                protection.active,
            );
            if index + 1 < protections.len() {
                ui::hairline(ui, theme);
            }
        }
    });
    card(ui, theme, "SECURE CONNECTIONS", |ui| {
        ui::row(
            ui,
            theme,
            crate::protections::HTTPS_ONLY_LABEL,
            ui::SWITCH,
            |ui| {
                ui::switch(
                    ui,
                    &mut next.https_only,
                    crate::protections::HTTPS_ONLY_LABEL,
                    theme,
                );
            },
        );
        ui::hairline(ui, theme);
        ui::row(ui, theme, "HTTP exceptions", ui::BUTTON, |ui| {
            if ui::button(ui, "Clear HTTP exceptions", theme, false).clicked() {
                *clear_exceptions = true;
            }
        });
    });
    card(ui, theme, "TRACKER BLOCKING", |ui| {
        ui::row_hint(
            ui,
            theme,
            "Block trackers",
            Some(crate::protections::TRACKER_SETTING_HINT),
            ui::SWITCH,
            |ui| {
                ui::switch(ui, &mut next.block_trackers, "Block trackers", theme);
            },
        );
    });
    card(ui, theme, "SITE ICONS", |ui| {
        ui::row(ui, theme, "Cached site icons", ui::BUTTON, |ui| {
            if ui::button(ui, "Clear site icons", theme, false).clicked() {
                *clear_icons = true;
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(width: f32, height: f32) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..egui::RawInput::default()
        }
    }

    fn no_id_warnings(output: &egui::FullOutput) {
        for clipped in &output.shapes {
            if let egui::Shape::Text(text) = &clipped.shape {
                let value = text.galley.text();
                assert!(
                    !(value.contains("use of") && value.contains(" ID")),
                    "{value}"
                );
            }
        }
    }

    #[test]
    fn smooth_scrolling_change_writes_the_field() {
        let mut settings = Settings::default();
        assert!(settings.smooth_scrolling);
        apply(&mut settings, Change::SmoothScrolling(false));
        assert!(!settings.smooth_scrolling);
        apply(&mut settings, Change::SmoothScrolling(true));
        assert!(settings.smooth_scrolling);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn hardware_acceleration_change_writes_the_field() {
        let mut settings = Settings::default();
        assert!(settings.hardware_acceleration);
        apply(&mut settings, Change::HardwareAcceleration(false));
        assert!(!settings.hardware_acceleration);
        apply(&mut settings, Change::HardwareAcceleration(true));
        assert!(settings.hardware_acceleration);
    }

    #[test]
    fn smooth_scrolling_hint_matches_the_platform() {
        let hint = smooth_scrolling_hint();
        #[cfg(target_os = "windows")]
        assert_eq!(hint, "Pages update after you restart Aegis");
        #[cfg(target_os = "linux")]
        assert_eq!(hint, "Applies to pages and Aegis");
        #[cfg(target_os = "macos")]
        assert_eq!(hint, "Applies to Aegis. Pages follow your system");
        assert!(!hint.is_empty());
    }

    #[test]
    fn nav_fill_steps_with_the_rows() {
        assert_eq!(NAV_PITCH, 34.0);
        let top = 56.0;
        for page in Page::ALL {
            let row = nav_row_top(top, page.index());
            assert_eq!(row, top + page.index() as f32 * 34.0);
            // the fill is placed with the same function, so it matches the row exactly
            assert_eq!(nav_row_top(top, page.index()), row);
        }
        let first = nav_row_top(top, 0);
        let last = nav_row_top(top, 4);
        assert_eq!(last - first, 4.0 * 34.0);
        assert_eq!(nav_row_top(top, 1) - nav_row_top(top, 0), 34.0);
    }

    #[test]
    fn sidebar_page_lays_out_every_row_and_card() {
        for (width, height, scale) in [
            (1280.0, 800.0, 1.25),
            (1280.0, 800.0, 1.0),
            (2000.0, 1067.0, 1.0),
            (900.0, 600.0, 1.0),
        ] {
            let settings = Settings::default();
            let theme = theme::resolve(&settings, false);
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            // the scale applies from the next frame
            ctx.run_ui(input(width, height), |_| {})
                .textures_delta
                .clear();
            let mut state = SettingsPage::with_page(Page::Sidebar);
            let mut passwords =
                passwords::Passwords::unavailable(std::path::PathBuf::new(), String::new());
            let mut imports = imports::Imports::new();
            for _ in 0..3 {
                let mut output = ctx.run_ui(input(width, height), |ui| {
                    let _ = show(
                        ui,
                        ui.max_rect(),
                        &settings,
                        theme,
                        &mut state,
                        &mut passwords,
                        &mut imports,
                    );
                });
                output.textures_delta.clear();
            }
            let laid = state.sidebar_laid;
            let pane = crate::app::pane_layout_at_scale(
                Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height)),
                220.0,
                settings.sidebar_side,
                scale,
            )
            .page_rect;
            for card in laid.cards {
                assert!(card.height() > 0.0, "{width}x{height}: {card:?}");
                assert!(
                    card.left() >= pane.left() && card.right() <= pane.right(),
                    "{width}x{height}: {card:?} outside {pane:?}"
                );
            }
            assert!(laid.cards[0].bottom() <= laid.cards[1].top());
            assert!(laid.cards[1].bottom() <= laid.cards[2].top());
            for (index, row) in laid.rows.into_iter().enumerate() {
                assert!(row.height() > 0.0, "{width}x{height}: row {index}");
                assert!(
                    laid.cards[0].contains_rect(row),
                    "{width}x{height}: row {index}"
                );
            }
            assert_eq!(laid.rows[4].top() - laid.rows[0].top(), 4.0 * 38.0);
        }
    }

    #[test]
    fn all_pages_fit_and_have_unique_ids() {
        for (width, height) in [(800.0, 600.0), (1280.0, 800.0), (1920.0, 1080.0)] {
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                for side in Side::ALL {
                    for page in [
                        Page::Appearance,
                        Page::Sidebar,
                        Page::Privacy,
                        Page::Passwords,
                        Page::Import,
                    ] {
                        let settings = Settings {
                            theme: mode,
                            sidebar_side: side,
                            ..Settings::default()
                        };
                        let theme = theme::resolve(&settings, mode == ThemeMode::Dark);
                        let ctx = egui::Context::default();
                        ctx.options_mut(|options| options.warn_on_id_clash = true);
                        let mut state = SettingsPage::with_page(page);
                        let mut passwords = passwords::Passwords::unavailable(
                            std::path::PathBuf::new(),
                            String::new(),
                        );
                        let mut imports = imports::Imports::new();
                        for frame_index in 0..3 {
                            let mut output = ctx.run_ui(input(width, height), |ui| {
                                let _ = show(
                                    ui,
                                    ui.max_rect(),
                                    &settings,
                                    theme,
                                    &mut state,
                                    &mut passwords,
                                    &mut imports,
                                );
                            });
                            if frame_index > 0 {
                                no_id_warnings(&output);
                                let pane = crate::app::pane_layout(
                                    Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        egui::vec2(width, height),
                                    ),
                                    220.0,
                                    settings.sidebar_side,
                                    8.0,
                                )
                                .page_rect;
                                let mut checked = 0;
                                for shape in &output.shapes {
                                    if pane.contains_rect(shape.clip_rect) {
                                        if let egui::Shape::Mesh(mesh) = &shape.shape
                                            && mesh
                                                .vertices
                                                .iter()
                                                .all(|vertex| vertex.color.a() == 0)
                                        {
                                            continue;
                                        }
                                        let visual = shape.shape.visual_bounding_rect();
                                        if visual.is_positive() {
                                            checked += 1;
                                            assert!(
                                                visual.left() >= pane.left() - 1.0
                                                    && visual.right() <= pane.right() + 1.0,
                                                "{page:?} {width}x{height}: {visual:?} outside {pane:?}"
                                            );
                                        }
                                    }
                                }
                                assert!(checked > 0, "no {page:?} shapes were checked");
                            }
                            output.textures_delta.clear();
                        }
                    }
                }
            }
        }
    }
}
