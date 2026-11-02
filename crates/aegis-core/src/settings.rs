use serde::{Deserialize, Serialize};

/// tab layout in the app
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// sidebar layout
    #[default]
    Vertical,
    /// tab strip layout
    Horizontal,
}

impl Layout {
    pub const ALL: [Self; 2] = [Self::Vertical, Self::Horizontal];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Vertical => "Sidebar",
            Self::Horizontal => "Classic tabs",
        }
    }
}

/// sidebar position
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    #[default]
    Left,
    Right,
}

impl Side {
    pub const ALL: [Self; 2] = [Self::Left, Self::Right];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

/// interface theme
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn label(&self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// interface accent color
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accent {
    #[default]
    Indigo,
    Teal,
    Green,
    Orange,
    Rose,
    Violet,
    Graphite,
    Custom,
}

impl Accent {
    pub const ALL: [Self; 7] = [
        Self::Indigo,
        Self::Teal,
        Self::Green,
        Self::Orange,
        Self::Rose,
        Self::Violet,
        Self::Graphite,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Indigo => "Indigo",
            Self::Teal => "Teal",
            Self::Green => "Green",
            Self::Orange => "Orange",
            Self::Rose => "Rose",
            Self::Violet => "Violet",
            Self::Graphite => "Graphite",
            Self::Custom => "Custom",
        }
    }

    pub fn rgb(&self) -> [u8; 3] {
        match self {
            Self::Indigo => [0x3B, 0x5B, 0xDB],
            Self::Teal => [0x0C, 0x85, 0x99],
            Self::Green => [0x2B, 0x8A, 0x3E],
            Self::Orange => [0xD9, 0x48, 0x0F],
            Self::Rose => [0xC2, 0x25, 0x5C],
            Self::Violet => [0x70, 0x48, 0xE8],
            Self::Graphite => [0x49, 0x50, 0x57],
            Self::Custom => [0x3B, 0x5B, 0xDB],
        }
    }
}

/// window glass style
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Glass {
    Clear,
    #[default]
    Tinted,
    Solid,
}

impl Glass {
    pub const ALL: [Self; 3] = [Self::Clear, Self::Tinted, Self::Solid];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Tinted => "Tinted",
            Self::Solid => "Solid",
        }
    }
}

/// interface corner style
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Corners {
    #[default]
    Round,
    Soft,
    Square,
}

impl Corners {
    pub const ALL: [Self; 3] = [Self::Round, Self::Soft, Self::Square];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Round => "Round",
            Self::Soft => "Soft",
            Self::Square => "Square",
        }
    }
}

/// pinned site shape
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinnedStyle {
    #[default]
    Circles,
    Tiles,
}

impl PinnedStyle {
    pub const ALL: [Self; 2] = [Self::Circles, Self::Tiles];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Circles => "Circles",
            Self::Tiles => "Tiles",
        }
    }
}

/// interface spacing
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

impl Density {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Comfortable, Self::Spacious];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
            Self::Spacious => "Spacious",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Setup {
    #[default]
    Explorer,
    Focus,
    Power,
    Minimal,
    Classic,
    Custom,
}

impl Setup {
    pub const ALL: [Self; 6] = [
        Self::Explorer,
        Self::Focus,
        Self::Power,
        Self::Minimal,
        Self::Classic,
        Self::Custom,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::Focus => "Focus",
            Self::Power => "Power",
            Self::Minimal => "Minimal",
            Self::Classic => "Classic",
            Self::Custom => "Custom",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SectionKind {
    Spaces,
    Pinned,
    Tabs,
    NewTab,
    Capsule,
}

impl SectionKind {
    pub const ALL: [Self; 5] = [
        Self::Spaces,
        Self::Pinned,
        Self::Tabs,
        Self::NewTab,
        Self::Capsule,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Spaces => "Spaces",
            Self::Pinned => "Pinned",
            Self::Tabs => "Tabs",
            Self::NewTab => "New Tab",
            Self::Capsule => "Address capsule",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Spaces => 0,
            Self::Pinned => 1,
            Self::Tabs => 2,
            Self::NewTab => 3,
            Self::Capsule => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionSlot {
    pub kind: SectionKind,
    pub visible: bool,
}

impl SectionSlot {
    pub const fn new(kind: SectionKind, visible: bool) -> Self {
        Self { kind, visible }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabLabel {
    #[default]
    Title,
    Site,
    Both,
}
impl TabLabel {
    pub const ALL: [Self; 3] = [Self::Title, Self::Site, Self::Both];
    pub fn label(&self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Site => "Site",
            Self::Both => "Both",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabIcons {
    #[default]
    Circles,
    Squares,
    Off,
}
impl TabIcons {
    pub const ALL: [Self; 3] = [Self::Circles, Self::Squares, Self::Off];
    pub fn label(&self) -> &'static str {
        match self {
            Self::Circles => "Circles",
            Self::Squares => "Squares",
            Self::Off => "Off",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseButton {
    #[default]
    OnHover,
    Always,
    Never,
}
impl CloseButton {
    pub const ALL: [Self; 3] = [Self::OnHover, Self::Always, Self::Never];
    pub fn label(&self) -> &'static str {
        match self {
            Self::OnHover => "On hover",
            Self::Always => "Always",
            Self::Never => "Never",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpacesStyle {
    #[default]
    Segmented,
    Dots,
    Icons,
}
impl SpacesStyle {
    pub const ALL: [Self; 3] = [Self::Segmented, Self::Dots, Self::Icons];
    pub fn label(&self) -> &'static str {
        match self {
            Self::Segmented => "Segmented",
            Self::Dots => "Dots",
            Self::Icons => "Icons",
        }
    }
}

/// animation preference
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotionPref {
    #[default]
    System,
    Full,
    Reduced,
}

impl MotionPref {
    pub const ALL: [Self; 3] = [Self::System, Self::Full, Self::Reduced];

    pub fn label(&self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Full => "Full",
            Self::Reduced => "Reduced",
        }
    }
}

/// frame pacing preference
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameLimit {
    #[default]
    Display,
    Vsync,
    Custom,
    Uncapped,
}

impl FrameLimit {
    pub const ALL: [Self; 4] = [Self::Display, Self::Vsync, Self::Custom, Self::Uncapped];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Display => "Match display",
            Self::Vsync => "VSync",
            Self::Custom => "Custom",
            Self::Uncapped => "Uncapped",
        }
    }
}

/// built-in search providers
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    /// duckduckgo search
    #[default]
    DuckDuckGo,
    /// brave search
    Brave,
    /// startpage search
    Startpage,
    /// google search
    Google,
    /// bing search
    Bing,
}

impl SearchEngine {
    /// returns a search URL with the query encoded as UTF-8 bytes
    pub fn query_url(&self, query: &str) -> String {
        let base = match self {
            Self::DuckDuckGo => "https://duckduckgo.com/?q=",
            Self::Brave => "https://search.brave.com/search?q=",
            Self::Startpage => "https://www.startpage.com/sp/search?query=",
            Self::Google => "https://www.google.com/search?q=",
            Self::Bing => "https://www.bing.com/search?q=",
        };
        let mut url = String::from(base);
        for byte in query.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                url.push(char::from(byte));
            } else {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                url.push('%');
                url.push(char::from(HEX[usize::from(byte >> 4)]));
                url.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
        url
    }
}

/// browser preferences saved in the session
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// tab layout
    pub layout: Layout,
    #[serde(default)]
    pub setup: Setup,
    #[serde(default = "default_sidebar_sections")]
    pub sidebar_sections: Vec<SectionSlot>,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: u16,
    #[serde(default = "default_sidebar_opacity")]
    pub sidebar_opacity: u8,
    #[serde(default)]
    pub tab_label: TabLabel,
    #[serde(default)]
    pub tab_icons: TabIcons,
    #[serde(default)]
    pub close_button: CloseButton,
    #[serde(default = "default_pinned_per_row")]
    pub pinned_per_row: u8,
    #[serde(default)]
    pub spaces_style: SpacesStyle,
    #[serde(default)]
    pub sidebar_side: Side,
    #[serde(default = "default_floating_page")]
    pub floating_page: bool,
    #[serde(default)]
    pub theme: ThemeMode,
    #[serde(default)]
    pub accent: Accent,
    #[serde(default = "default_custom_accent")]
    pub custom_accent: [u8; 3],
    #[serde(default)]
    pub glass: Glass,
    #[serde(default = "default_noise")]
    pub noise: u8,
    #[serde(default)]
    pub corners: Corners,
    #[serde(default)]
    pub pinned_style: PinnedStyle,
    #[serde(default)]
    pub density: Density,
    #[serde(default = "default_text_scale")]
    pub text_scale: u16,
    #[serde(default)]
    pub motion: MotionPref,
    #[serde(default)]
    pub frame_limit: FrameLimit,
    #[serde(default = "default_custom_fps")]
    pub custom_fps: u16,
    #[serde(default = "default_smooth_scrolling")]
    pub smooth_scrolling: bool,
    #[serde(default = "default_hardware_acceleration")]
    pub hardware_acceleration: bool,
    #[serde(default)]
    pub welcome_done: bool,
    /// compact sidebar mode
    pub compact: bool,
    /// idle minutes before suspension
    pub suspend_after_minutes: u32,
    /// idle minutes before discarding
    pub discard_after_minutes: u32,
    /// search provider
    pub search_engine: SearchEngine,
    /// upgrade plain HTTP navigation
    pub https_only: bool,
    /// block known trackers
    pub block_trackers: bool,
    /// block third-party cookies
    pub block_third_party_cookies: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            layout: Layout::Vertical,
            setup: Setup::Explorer,
            sidebar_sections: default_sidebar_sections(),
            sidebar_width: 272,
            sidebar_opacity: 80,
            tab_label: TabLabel::Title,
            tab_icons: TabIcons::Circles,
            close_button: CloseButton::OnHover,
            pinned_per_row: 5,
            spaces_style: SpacesStyle::Segmented,
            sidebar_side: Side::Left,
            floating_page: true,
            theme: ThemeMode::System,
            accent: Accent::Indigo,
            custom_accent: default_custom_accent(),
            glass: Glass::Tinted,
            noise: 25,
            corners: Corners::Round,
            pinned_style: PinnedStyle::Circles,
            density: Density::Comfortable,
            text_scale: 100,
            motion: MotionPref::System,
            frame_limit: FrameLimit::Display,
            custom_fps: 120,
            smooth_scrolling: true,
            hardware_acceleration: true,
            welcome_done: false,
            compact: false,
            suspend_after_minutes: 10,
            discard_after_minutes: 60,
            search_engine: SearchEngine::DuckDuckGo,
            https_only: true,
            block_trackers: true,
            block_third_party_cookies: true,
        }
    }
}

impl Settings {
    pub fn accent_rgb(&self) -> [u8; 3] {
        if self.accent == Accent::Custom {
            self.custom_accent
        } else {
            self.accent.rgb()
        }
    }

    pub fn apply_setup(&mut self, setup: Setup) {
        if setup == Setup::Custom {
            self.setup = setup;
            return;
        }
        self.layout = if setup == Setup::Classic {
            Layout::Horizontal
        } else {
            Layout::Vertical
        };
        self.sidebar_side = if setup == Setup::Power {
            Side::Right
        } else {
            Side::Left
        };
        self.sidebar_width = match setup {
            Setup::Focus => 260,
            Setup::Power => 248,
            Setup::Minimal => 240,
            _ => 272,
        };
        self.compact = setup == Setup::Focus;
        self.tab_label = if setup == Setup::Power {
            TabLabel::Both
        } else {
            TabLabel::Title
        };
        self.tab_icons = match setup {
            Setup::Power => TabIcons::Squares,
            Setup::Minimal => TabIcons::Off,
            _ => TabIcons::Circles,
        };
        self.close_button = if setup == Setup::Power {
            CloseButton::Always
        } else {
            CloseButton::OnHover
        };
        self.density = if setup == Setup::Power {
            Density::Compact
        } else {
            Density::Comfortable
        };
        self.pinned_style = if setup == Setup::Power {
            PinnedStyle::Tiles
        } else {
            PinnedStyle::Circles
        };
        self.pinned_per_row = match setup {
            Setup::Power => 6,
            Setup::Minimal => 4,
            _ => 5,
        };
        self.spaces_style = match setup {
            Setup::Power => SpacesStyle::Icons,
            Setup::Focus | Setup::Minimal => SpacesStyle::Dots,
            _ => SpacesStyle::Segmented,
        };
        self.sidebar_sections = match setup {
            Setup::Focus => vec![
                SectionSlot::new(SectionKind::Tabs, true),
                SectionSlot::new(SectionKind::NewTab, true),
                SectionSlot::new(SectionKind::Capsule, true),
                SectionSlot::new(SectionKind::Spaces, false),
                SectionSlot::new(SectionKind::Pinned, false),
            ],
            Setup::Power => vec![
                SectionSlot::new(SectionKind::Capsule, true),
                SectionSlot::new(SectionKind::Spaces, true),
                SectionSlot::new(SectionKind::Pinned, true),
                SectionSlot::new(SectionKind::Tabs, true),
                SectionSlot::new(SectionKind::NewTab, false),
            ],
            Setup::Minimal => vec![
                SectionSlot::new(SectionKind::Capsule, true),
                SectionSlot::new(SectionKind::Tabs, true),
                SectionSlot::new(SectionKind::NewTab, true),
                SectionSlot::new(SectionKind::Spaces, false),
                SectionSlot::new(SectionKind::Pinned, false),
            ],
            _ => default_sidebar_sections(),
        };
        self.setup = setup;
    }

    pub(crate) fn normalized(mut self) -> Self {
        self.custom_fps = self.custom_fps.clamp(30, 500);
        self.suspend_after_minutes = self.suspend_after_minutes.max(1);
        self.discard_after_minutes = self.discard_after_minutes.max(self.suspend_after_minutes);
        let clamped = self.text_scale.clamp(85, 130);
        self.text_scale = ((clamped + 2) / 5) * 5;
        self.noise = self.noise.min(100);
        self.sidebar_opacity = self.sidebar_opacity.clamp(30, 100);
        self.sidebar_width = ((self.sidebar_width.clamp(220, 360) + 2) / 4) * 4;
        self.pinned_per_row = self.pinned_per_row.clamp(4, 6);
        let mut seen = [false; 5];
        self.sidebar_sections.retain(|slot| {
            let index = slot.kind.index();
            if seen[index] {
                false
            } else {
                seen[index] = true;
                true
            }
        });
        for (index, kind) in SectionKind::ALL.iter().enumerate() {
            if !seen[index] {
                self.sidebar_sections.push(SectionSlot::new(*kind, true));
            }
        }
        self
    }
}

fn default_custom_fps() -> u16 {
    120
}

fn default_sidebar_sections() -> Vec<SectionSlot> {
    SectionKind::ALL
        .iter()
        .map(|kind| SectionSlot::new(*kind, true))
        .collect()
}

fn default_custom_accent() -> [u8; 3] {
    Accent::Indigo.rgb()
}

fn default_sidebar_width() -> u16 {
    272
}

fn default_sidebar_opacity() -> u8 {
    80
}
fn default_pinned_per_row() -> u8 {
    5
}

fn default_floating_page() -> bool {
    true
}

fn default_smooth_scrolling() -> bool {
    true
}

fn default_hardware_acceleration() -> bool {
    true
}

fn default_text_scale() -> u16 {
    100
}

fn default_noise() -> u8 {
    25
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding() {
        assert_eq!(
            SearchEngine::DuckDuckGo.query_url("a b&#é"),
            "https://duckduckgo.com/?q=a%20b%26%23%C3%A9"
        );
        assert_eq!(
            SearchEngine::Brave.query_url("azAZ09-_.~"),
            "https://search.brave.com/search?q=azAZ09-_.~"
        );
    }

    #[test]
    fn old_settings_use_defaults() {
        let settings: Settings = serde_json::from_str("{\"compact\":true}").unwrap();
        assert!(settings.compact);
        assert_eq!(settings.suspend_after_minutes, 10);
        assert_eq!(settings.sidebar_side, Side::Left);
        assert!(settings.floating_page);
        assert_eq!(settings.theme, ThemeMode::System);
        assert_eq!(settings.accent, Accent::Indigo);
        assert_eq!(settings.custom_accent, Accent::Indigo.rgb());
        assert_eq!(settings.glass, Glass::Tinted);
        assert_eq!(settings.noise, 25);
        assert_eq!(settings.corners, Corners::Round);
        assert_eq!(settings.pinned_style, PinnedStyle::Circles);
        assert_eq!(settings.density, Density::Comfortable);
        assert_eq!(settings.text_scale, 100);
        assert_eq!(settings.motion, MotionPref::System);
        assert_eq!(settings.frame_limit, FrameLimit::Display);
        assert_eq!(settings.custom_fps, 120);
        assert!(settings.smooth_scrolling);
        assert!(settings.hardware_acceleration);
        assert!(!settings.welcome_done);
        assert_eq!(settings.setup, Setup::Explorer);
        assert_eq!(settings.sidebar_sections, default_sidebar_sections());
        assert_eq!(settings.sidebar_width, 272);
        assert_eq!(settings.sidebar_opacity, 80);
        assert_eq!(settings.tab_label, TabLabel::Title);
        assert_eq!(settings.tab_icons, TabIcons::Circles);
        assert_eq!(settings.close_button, CloseButton::OnHover);
        assert_eq!(settings.pinned_per_row, 5);
        assert_eq!(settings.spaces_style, SpacesStyle::Segmented);
    }

    #[test]
    fn text_scale_normalizes_on_set_settings() {
        use crate::{Browser, Command};

        let mut browser = Browser::new();
        for (input, expected) in [(84, 85), (131, 130), (102, 100), (103, 105)] {
            browser
                .apply(
                    Command::SetSettings(Settings {
                        text_scale: input,
                        ..Settings::default()
                    }),
                    0,
                )
                .unwrap();
            assert_eq!(browser.settings().text_scale, expected);
        }
    }

    #[test]
    fn noise_normalizes_on_set_settings() {
        use crate::{Browser, Command};

        let mut browser = Browser::new();
        browser
            .apply(
                Command::SetSettings(Settings {
                    noise: 150,
                    ..Settings::default()
                }),
                0,
            )
            .unwrap();
        assert_eq!(browser.settings().noise, 100);
    }

    #[test]
    fn custom_fps_normalizes_on_set_settings() {
        use crate::{Browser, Command};

        let mut browser = Browser::new();
        for (input, expected) in [(0, 30), (29, 30), (500, 500), (501, 500)] {
            browser
                .apply(
                    Command::SetSettings(Settings {
                        custom_fps: input,
                        ..Settings::default()
                    }),
                    0,
                )
                .unwrap();
            assert_eq!(browser.settings().custom_fps, expected);
        }
    }

    #[test]
    fn appearance_labels_cover_variants() {
        assert_eq!(
            Layout::ALL.map(|value| value.label()),
            ["Sidebar", "Classic tabs"]
        );
        assert_eq!(Side::ALL.map(|value| value.label()), ["Left", "Right"]);
        assert_eq!(
            ThemeMode::ALL.map(|value| value.label()),
            ["System", "Light", "Dark"]
        );
        assert_eq!(
            Accent::ALL.map(|value| value.label()),
            [
                "Indigo", "Teal", "Green", "Orange", "Rose", "Violet", "Graphite"
            ]
        );
        assert_eq!(
            Glass::ALL.map(|value| value.label()),
            ["Clear", "Tinted", "Solid"]
        );
        assert_eq!(
            Corners::ALL.map(|value| value.label()),
            ["Round", "Soft", "Square"]
        );
        assert_eq!(
            PinnedStyle::ALL.map(|value| value.label()),
            ["Circles", "Tiles"]
        );
        assert_eq!(
            Density::ALL.map(|value| value.label()),
            ["Compact", "Comfortable", "Spacious"]
        );
        assert_eq!(
            MotionPref::ALL.map(|value| value.label()),
            ["System", "Full", "Reduced"]
        );
        assert_eq!(
            FrameLimit::ALL.map(|value| value.label()),
            ["Match display", "VSync", "Custom", "Uncapped"]
        );
        assert_eq!(
            Accent::ALL.map(|value| value.rgb()),
            [
                [0x3B, 0x5B, 0xDB],
                [0x0C, 0x85, 0x99],
                [0x2B, 0x8A, 0x3E],
                [0xD9, 0x48, 0x0F],
                [0xC2, 0x25, 0x5C],
                [0x70, 0x48, 0xE8],
                [0x49, 0x50, 0x57],
            ]
        );
    }

    #[test]
    fn appearance_settings_round_trip() {
        let settings = Settings {
            layout: Layout::Horizontal,
            sidebar_side: Side::Right,
            floating_page: false,
            theme: ThemeMode::Dark,
            accent: Accent::Rose,
            glass: Glass::Solid,
            corners: Corners::Square,
            pinned_style: PinnedStyle::Tiles,
            density: Density::Spacious,
            text_scale: 125,
            motion: MotionPref::Reduced,
            welcome_done: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, settings);
    }

    #[test]
    fn setups_match_design_table() {
        let expected = [
            (
                Setup::Explorer,
                "SPTNC",
                Side::Left,
                272,
                false,
                TabLabel::Title,
                TabIcons::Circles,
                CloseButton::OnHover,
                Density::Comfortable,
                PinnedStyle::Circles,
                5,
                SpacesStyle::Segmented,
                Layout::Vertical,
            ),
            (
                Setup::Focus,
                "TNCsp",
                Side::Left,
                260,
                true,
                TabLabel::Title,
                TabIcons::Circles,
                CloseButton::OnHover,
                Density::Comfortable,
                PinnedStyle::Circles,
                5,
                SpacesStyle::Dots,
                Layout::Vertical,
            ),
            (
                Setup::Power,
                "CSPTn",
                Side::Right,
                248,
                false,
                TabLabel::Both,
                TabIcons::Squares,
                CloseButton::Always,
                Density::Compact,
                PinnedStyle::Tiles,
                6,
                SpacesStyle::Icons,
                Layout::Vertical,
            ),
            (
                Setup::Minimal,
                "CTNsp",
                Side::Left,
                240,
                false,
                TabLabel::Title,
                TabIcons::Off,
                CloseButton::OnHover,
                Density::Comfortable,
                PinnedStyle::Circles,
                4,
                SpacesStyle::Dots,
                Layout::Vertical,
            ),
            (
                Setup::Classic,
                "SPTNC",
                Side::Left,
                272,
                false,
                TabLabel::Title,
                TabIcons::Circles,
                CloseButton::OnHover,
                Density::Comfortable,
                PinnedStyle::Circles,
                5,
                SpacesStyle::Segmented,
                Layout::Horizontal,
            ),
        ];
        for (
            setup,
            sections,
            side,
            width,
            compact,
            label,
            icons,
            close,
            density,
            pinned,
            per_row,
            spaces,
            layout,
        ) in expected
        {
            let mut settings = Settings::default();
            settings.apply_setup(setup);
            let actual: String = settings
                .sidebar_sections
                .iter()
                .map(|slot| match (slot.kind, slot.visible) {
                    (SectionKind::Spaces, true) => 'S',
                    (SectionKind::Spaces, false) => 's',
                    (SectionKind::Pinned, true) => 'P',
                    (SectionKind::Pinned, false) => 'p',
                    (SectionKind::Tabs, true) => 'T',
                    (SectionKind::Tabs, false) => 't',
                    (SectionKind::NewTab, true) => 'N',
                    (SectionKind::NewTab, false) => 'n',
                    (SectionKind::Capsule, true) => 'C',
                    (SectionKind::Capsule, false) => 'c',
                })
                .collect();
            assert_eq!(actual, sections, "{setup:?}");
            assert_eq!(
                (
                    settings.setup,
                    settings.sidebar_side,
                    settings.sidebar_width,
                    settings.compact,
                    settings.tab_label,
                    settings.tab_icons,
                    settings.close_button,
                    settings.density,
                    settings.pinned_style,
                    settings.pinned_per_row,
                    settings.spaces_style,
                    settings.layout
                ),
                (
                    setup, side, width, compact, label, icons, close, density, pinned, per_row,
                    spaces, layout
                )
            );
        }
        let mut custom = Settings {
            sidebar_width: 320,
            ..Settings::default()
        };
        custom.apply_setup(Setup::Custom);
        assert_eq!(custom.setup, Setup::Custom);
        assert_eq!(custom.sidebar_width, 320);
    }

    #[test]
    fn sidebar_settings_normalize_on_set_settings() {
        use crate::{Browser, Command};
        let mut browser = Browser::new();
        let settings = Settings {
            sidebar_width: 221,
            pinned_per_row: 9,
            sidebar_sections: vec![
                SectionSlot::new(SectionKind::Capsule, false),
                SectionSlot::new(SectionKind::Capsule, true),
                SectionSlot::new(SectionKind::Tabs, true),
            ],
            ..Settings::default()
        };
        browser.apply(Command::SetSettings(settings), 0).unwrap();
        let result = browser.settings();
        assert_eq!(result.sidebar_width, 220);
        assert_eq!(result.pinned_per_row, 6);
        assert_eq!(
            result.sidebar_sections,
            vec![
                SectionSlot::new(SectionKind::Capsule, false),
                SectionSlot::new(SectionKind::Tabs, true),
                SectionSlot::new(SectionKind::Spaces, true),
                SectionSlot::new(SectionKind::Pinned, true),
                SectionSlot::new(SectionKind::NewTab, true)
            ]
        );
        let mut settings = result.clone();
        settings.sidebar_width = 359;
        settings.pinned_per_row = 1;
        browser.apply(Command::SetSettings(settings), 0).unwrap();
        assert_eq!(browser.settings().sidebar_width, 360);
        assert_eq!(browser.settings().pinned_per_row, 4);
    }

    #[test]
    fn sidebar_opacity_normalizes_and_setups_preserve_it() {
        use crate::{Browser, Command};
        let old_session: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old_session.sidebar_opacity, 80);
        let mut browser = Browser::new();
        for (input, expected) in [(0, 30), (30, 30), (100, 100), (255, 100)] {
            let mut settings = browser.settings().clone();
            settings.sidebar_opacity = input;
            browser.apply(Command::SetSettings(settings), 0).unwrap();
            assert_eq!(browser.settings().sidebar_opacity, expected);
        }
        browser.apply(Command::ApplySetup(Setup::Focus), 0).unwrap();
        assert_eq!(browser.settings().sidebar_opacity, 100);
    }
}
