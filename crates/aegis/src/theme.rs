use aegis_core::GroupColor;
use eframe::egui::{self, Color32};

pub const SIDEBAR_WIDTH: f32 = 260.0;
pub const PADDING: f32 = 10.0;
pub const ROW_HEIGHT: f32 = 32.0;
pub const HOT_STRIP_WIDTH: f32 = 6.0;

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub separator: Color32,
    pub border: Color32,
}

pub fn palette(ctx: &egui::Context) -> Palette {
    if ctx.theme() == egui::Theme::Dark {
        Palette {
            background: Color32::from_rgb(28, 28, 31),
            hover: Color32::from_rgb(42, 42, 46),
            active: Color32::from_rgb(52, 52, 58),
            text: Color32::from_rgb(232, 232, 234),
            secondary: Color32::from_rgb(154, 154, 162),
            separator: Color32::from_rgb(46, 46, 51),
            border: Color32::TRANSPARENT,
        }
    } else {
        Palette {
            background: Color32::from_rgb(243, 243, 245),
            hover: Color32::from_rgb(230, 230, 234),
            active: Color32::WHITE,
            text: Color32::from_rgb(30, 30, 34),
            secondary: Color32::from_rgb(107, 107, 115),
            separator: Color32::from_rgb(222, 222, 227),
            border: Color32::from_rgb(218, 218, 223),
        }
    }
}

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

pub fn group_color(color: GroupColor) -> Color32 {
    match color {
        GroupColor::Grey => Color32::from_rgb(139, 143, 152),
        GroupColor::Blue => Color32::from_rgb(100, 153, 224),
        GroupColor::Red => Color32::from_rgb(213, 107, 111),
        GroupColor::Yellow => Color32::from_rgb(210, 174, 90),
        GroupColor::Green => Color32::from_rgb(111, 177, 132),
        GroupColor::Pink => Color32::from_rgb(210, 129, 165),
        GroupColor::Purple => Color32::from_rgb(163, 132, 201),
        GroupColor::Cyan => Color32::from_rgb(104, 178, 190),
        GroupColor::Orange => Color32::from_rgb(215, 146, 99),
    }
}
