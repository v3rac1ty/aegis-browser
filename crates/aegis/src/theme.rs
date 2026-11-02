use aegis_core::{Corners, Density, GroupColor, MotionPref, Settings, ThemeMode};
use eframe::egui::{self, Color32};

pub const HOT_STRIP_WIDTH: f32 = 6.0;
pub const GROUP_COLORS: [GroupColor; 9] = [
    GroupColor::Grey,
    GroupColor::Blue,
    GroupColor::Red,
    GroupColor::Yellow,
    GroupColor::Green,
    GroupColor::Pink,
    GroupColor::Purple,
    GroupColor::Cyan,
    GroupColor::Orange,
];

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub text: Color32,
    pub secondary: Color32,
    pub tertiary: Color32,
    pub hover: Color32,
    pub pane: Color32,
    pub pane_edge: Color32,
    pub pane_highlight: Color32,
    pub well: Color32,
    pub seg_on: Color32,
    pub row_on: Color32,
    pub hairline: Color32,
    pub capsule: Color32,
    pub page: Color32,
    pub content: Color32,
    pub card: Color32,
    pub card_edge: Color32,
    pub overlay: Color32,
    pub glass_tint_clear: Color32,
    pub glass_tint_solid: Color32,
}

impl Palette {
    pub fn light() -> Self {
        Self {
            text: rgb(0x1c1c1f),
            secondary: rgba(0x3c3c43, 0.66),
            tertiary: rgba(0x3c3c43, 0.34),
            hover: rgba(0xffffff, 0.45),
            pane: rgba(0xffffff, 0.40),
            pane_edge: rgba(0xffffff, 0.55),
            pane_highlight: rgba(0xffffff, 0.90),
            well: rgba(0x000000, 0.06),
            seg_on: rgba(0xffffff, 0.92),
            row_on: rgba(0xffffff, 0.78),
            hairline: rgba(0x000000, 0.07),
            capsule: rgba(0xffffff, 0.55),
            page: rgb(0xffffff),
            content: rgba(0xffffff, 0.72),
            card: rgb(0xffffff),
            card_edge: rgba(0x000000, 0.05),
            overlay: rgba(0xffffff, 0.66),
            glass_tint_clear: Color32::TRANSPARENT,
            glass_tint_solid: rgb(0xe6e9f0),
        }
    }

    pub fn dark() -> Self {
        Self {
            text: rgb(0xf3f3f6),
            secondary: rgba(0xebebf5, 0.62),
            tertiary: rgba(0xebebf5, 0.32),
            hover: rgba(0xffffff, 0.08),
            pane: rgba(0x28282e, 0.42),
            pane_edge: rgba(0xffffff, 0.10),
            pane_highlight: rgba(0xffffff, 0.14),
            well: rgba(0x000000, 0.22),
            seg_on: rgba(0xffffff, 0.16),
            row_on: rgba(0xffffff, 0.13),
            hairline: rgba(0xffffff, 0.10),
            capsule: rgba(0x000000, 0.22),
            page: rgb(0x141417),
            content: rgba(0x18181c, 0.86),
            card: rgba(0xffffff, 0.06),
            card_edge: rgba(0xffffff, 0.08),
            overlay: rgba(0x24242a, 0.62),
            glass_tint_clear: Color32::TRANSPARENT,
            glass_tint_solid: rgb(0x1b1c22),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub window_padding: f32,
    pub pane_gap: f32,
    pub pane_radius: f32,
    pub row_radius: f32,
    pub control_radius: f32,
    pub overlay_radius: f32,
    pub row_height: f32,
    pub control_size: f32,
    pub sidebar_width: f32,
}

impl Metrics {
    pub fn from_settings(settings: &Settings) -> Self {
        let (pane_radius, row_radius, control_radius, overlay_radius) = match settings.corners {
            Corners::Round => (13.0, 10.0, 9.0, 18.0),
            Corners::Soft => (8.0, 7.0, 6.0, 12.0),
            Corners::Square => (2.0, 2.0, 2.0, 4.0),
        };
        let (row_height, control_size) = match settings.density {
            Density::Compact => (30.0, 26.0),
            Density::Comfortable => (34.0, 30.0),
            Density::Spacious => (40.0, 34.0),
        };
        Self {
            window_padding: 8.0,
            pane_gap: 8.0,
            pane_radius,
            row_radius,
            control_radius,
            overlay_radius,
            row_height,
            control_size,
            sidebar_width: f32::from(settings.sidebar_width),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub palette: Palette,
    pub pane_scrim: Color32,
    pub metrics: Metrics,
    pub accent: Color32,
    pub dark: bool,
    pub motion_pref: MotionPref,
}

pub fn resolve(settings: &Settings, system_dark: bool) -> Theme {
    let dark = match settings.theme {
        ThemeMode::System => system_dark,
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
    };
    let mut palette = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let (pane, pane_scrim, text, secondary) =
        sidebar_colors(palette.glass_tint_solid, settings.sidebar_opacity);
    palette.pane = pane;
    palette.text = text;
    palette.secondary = secondary;
    Theme {
        palette,
        pane_scrim,
        metrics: Metrics::from_settings(settings),
        accent: accent_color(settings),
        dark,
        motion_pref: settings.motion,
    }
}

pub fn apply_visuals(ctx: &egui::Context, theme: Theme) {
    let mut style = (*ctx.style_of(ctx.theme())).clone();
    let visuals = &mut style.visuals;
    visuals.override_text_color = Some(theme.palette.text);
    visuals.weak_text_color = Some(theme.palette.secondary);
    visuals.widgets.noninteractive.bg_fill = theme.palette.card;
    visuals.widgets.inactive.bg_fill = theme.palette.well;
    visuals.widgets.hovered.bg_fill = theme.palette.hover;
    visuals.widgets.active.bg_fill = theme.palette.seg_on;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
    ] {
        widget.fg_stroke.color = theme.palette.text;
        widget.bg_stroke.color = theme.palette.card_edge;
    }
    visuals.selection.bg_fill = theme.accent.gamma_multiply(0.25);
    visuals.selection.stroke.color = theme.accent;
    visuals.text_cursor.stroke.color = theme.accent;
    visuals.extreme_bg_color = theme.palette.well;
    visuals.text_edit_bg_color = Some(theme.palette.well);
    visuals.window_fill = theme.palette.overlay;
    visuals.panel_fill = theme.palette.content;
    style.spacing.scroll.floating = true;
    style.spacing.scroll.bar_width = 5.0;
    style.spacing.scroll.floating_width = 3.0;
    ctx.set_style_of(ctx.theme(), style);
}

/// animate scrolling in aegis ui, or jump straight to the target
pub fn apply_scroll_animation(ctx: &egui::Context, smooth: bool) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.scroll_animation = scroll_animation(smooth);
        ctx.set_style_of(theme, style);
    }
}

fn scroll_animation(smooth: bool) -> egui::style::ScrollAnimation {
    if smooth {
        egui::style::ScrollAnimation::default()
    } else {
        egui::style::ScrollAnimation::none()
    }
}

fn luminance(color: Color32) -> f32 {
    let channel = |value: u8| {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (bright, dark) = (
        luminance(a).max(luminance(b)),
        luminance(a).min(luminance(b)),
    );
    (bright + 0.05) / (dark + 0.05)
}

fn over(color: Color32, backdrop: u8, opacity: f32) -> Color32 {
    let channel = |value: u8| {
        (f32::from(value) * opacity + f32::from(backdrop) * (1.0 - opacity)).round() as u8
    };
    Color32::from_rgb(channel(color.r()), channel(color.g()), channel(color.b()))
}

fn worst_contrast(text: Color32, tint: Color32, opacity: f32) -> f32 {
    contrast(text, over(tint, 0, opacity)).min(contrast(text, over(tint, 255, opacity)))
}

pub fn sidebar_colors(tint: Color32, opacity: u8) -> (Color32, Color32, Color32, Color32) {
    let alpha = f32::from(opacity.clamp(30, 100)) / 100.0;
    let mut effective = alpha.max(0.75);
    let dark_text = Color32::from_rgb(28, 28, 31);
    let light_text = Color32::from_rgb(243, 243, 246);
    let primary = loop {
        let dark_score = worst_contrast(dark_text, tint, effective);
        let light_score = worst_contrast(light_text, tint, effective);
        if dark_score >= 4.5 {
            break dark_text;
        }
        if light_score >= 4.5 {
            break light_text;
        }
        effective = (effective + 0.01).min(1.0);
    };
    let candidate = if primary == dark_text {
        Color32::from_rgb(60, 60, 67)
    } else {
        Color32::from_rgb(200, 200, 208)
    };
    let secondary = if worst_contrast(candidate, tint, effective) >= 3.0 {
        candidate
    } else {
        primary
    };
    let fill = Color32::from_rgba_unmultiplied(
        tint.r(),
        tint.g(),
        tint.b(),
        (alpha * 255.0).round() as u8,
    );
    let scrim_alpha = if effective > alpha {
        (effective - alpha) / (1.0 - alpha)
    } else {
        0.0
    };
    let scrim = Color32::from_rgba_unmultiplied(
        tint.r(),
        tint.g(),
        tint.b(),
        (scrim_alpha * 255.0).round() as u8,
    );
    (fill, scrim, primary, secondary)
}

#[cfg(test)]
mod sidebar_color_tests {
    use super::*;

    #[test]
    fn text_contrast_across_opacities() {
        for tint in [
            Palette::light().glass_tint_solid,
            Palette::dark().glass_tint_solid,
        ] {
            for opacity in 30..=100 {
                let (pane, scrim, primary, secondary) = sidebar_colors(tint, opacity);
                assert_eq!(pane.a(), (f32::from(opacity) * 2.55).round() as u8);
                let effective = f32::from(pane.a()) / 255.0
                    + f32::from(scrim.a()) / 255.0 * (1.0 - f32::from(pane.a()) / 255.0);
                assert!(worst_contrast(primary, tint, effective) >= 4.5);
                assert!(worst_contrast(secondary, tint, effective) >= 3.0);
            }
        }
    }
}

pub fn accent_color(settings: &Settings) -> Color32 {
    let [r, g, b] = settings.accent_rgb();
    Color32::from_rgb(r, g, b)
}

pub fn group_color(color: GroupColor) -> Color32 {
    let value = match color {
        GroupColor::Grey => 0x868e96,
        GroupColor::Blue => 0x3b5bdb,
        GroupColor::Red => 0xe03131,
        GroupColor::Yellow => 0xf59f00,
        GroupColor::Green => 0x2b8a3e,
        GroupColor::Pink => 0xc2255c,
        GroupColor::Purple => 0x7048e8,
        GroupColor::Cyan => 0x0c8599,
        GroupColor::Orange => 0xd9480f,
    };
    rgb(value)
}

pub fn hairline(ctx: &egui::Context) -> f32 {
    1.0 / ctx.pixels_per_point()
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn rgba(hex: u32, alpha: f32) -> Color32 {
    let [r, g, b, _] = rgb(hex).to_array();
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis_core::Accent;

    #[test]
    fn scroll_animation_follows_the_setting() {
        let ctx = egui::Context::default();
        apply_scroll_animation(&ctx, false);
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).scroll_animation,
            egui::style::ScrollAnimation::none()
        );
        apply_scroll_animation(&ctx, true);
        assert_eq!(
            ctx.style_of(egui::Theme::Light).scroll_animation,
            egui::style::ScrollAnimation::default()
        );
    }

    #[test]
    fn metrics_match_spec() {
        for (corners, radii) in [
            (Corners::Round, [13.0, 10.0, 9.0, 18.0]),
            (Corners::Soft, [8.0, 7.0, 6.0, 12.0]),
            (Corners::Square, [2.0, 2.0, 2.0, 4.0]),
        ] {
            for (density, sizes) in [
                (Density::Compact, [30.0, 26.0, 248.0]),
                (Density::Comfortable, [34.0, 30.0, 272.0]),
                (Density::Spacious, [40.0, 34.0, 296.0]),
            ] {
                let settings = Settings {
                    corners,
                    density,
                    ..Settings::default()
                };
                let m = Metrics::from_settings(&settings);
                assert_eq!(
                    [
                        m.pane_radius,
                        m.row_radius,
                        m.control_radius,
                        m.overlay_radius
                    ],
                    radii
                );
                assert_eq!([m.row_height, m.control_size], [sizes[0], sizes[1]]);
                assert_eq!(m.sidebar_width, 272.0);
                assert_eq!([m.window_padding, m.pane_gap], [8.0, 8.0]);
            }
        }
    }

    #[test]
    fn resolves_theme_and_color_tables() {
        let mut settings = Settings::default();
        assert!(!resolve(&settings, false).dark);
        assert!(resolve(&settings, true).dark);
        settings.theme = ThemeMode::Light;
        assert!(!resolve(&settings, true).dark);
        settings.theme = ThemeMode::Dark;
        assert!(resolve(&settings, false).dark);
        let accents = [
            0x3b5bdb, 0x0c8599, 0x2b8a3e, 0xd9480f, 0xc2255c, 0x7048e8, 0x495057,
        ];
        for (accent, hex) in Accent::ALL.into_iter().zip(accents) {
            settings.accent = accent;
            assert_eq!(accent_color(&settings), rgb(hex));
        }
        let groups = [
            0x868e96, 0x3b5bdb, 0xe03131, 0xf59f00, 0x2b8a3e, 0xc2255c, 0x7048e8, 0x0c8599,
            0xd9480f,
        ];
        for (color, hex) in GROUP_COLORS.into_iter().zip(groups) {
            assert_eq!(group_color(color), rgb(hex));
        }
    }
}
