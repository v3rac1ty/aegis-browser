use std::path::PathBuf;

use eframe::egui::{self, Color32, FontData, FontFamily, FontId, TextStyle};

pub fn install(ctx: &egui::Context, text_scale: u16) {
    let regular = system_font();
    let semibold = semibold_font();
    let mut definitions = egui::FontDefinitions::default();
    if let Some(bytes) = regular {
        definitions
            .font_data
            .insert("system_ui".to_owned(), FontData::from_owned(bytes).into());
        definitions
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system_ui".to_owned());
    }
    let mut family = Vec::new();
    if let Some(bytes) = semibold {
        definitions.font_data.insert(
            "system_semibold".to_owned(),
            FontData::from_owned(bytes).into(),
        );
        family.push("system_semibold".to_owned());
    }
    family.extend(
        definitions
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    definitions
        .families
        .insert(FontFamily::Name("semibold".into()), family);
    ctx.set_fonts(definitions);
    apply_scale(ctx, text_scale);
}

pub fn apply_scale(ctx: &egui::Context, text_scale: u16) {
    let scale = f32::from(text_scale) / 100.0;
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*ctx.style_of(theme)).clone();
        for (name, size) in [
            ("Title", 20.0),
            ("Heading", 15.0),
            ("Input", 15.0),
            ("Body", 13.0),
            ("Label", 12.0),
            ("Caption", 11.0),
        ] {
            style.text_styles.insert(
                TextStyle::Name(name.into()),
                if matches!(name, "Title" | "Heading" | "Label") {
                    FontId::new(size * scale, FontFamily::Name("semibold".into()))
                } else {
                    FontId::proportional(size * scale)
                },
            );
        }
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(13.0 * scale));
        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(15.0 * scale, FontFamily::Name("semibold".into())),
        );
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(11.0 * scale));
        ctx.set_style_of(theme, style);
    }
}

pub fn id(ctx: &egui::Context, name: &str) -> FontId {
    ctx.style_of(ctx.theme())
        .text_styles
        .get(&TextStyle::Name(name.into()))
        .cloned()
        .unwrap_or_else(|| FontId::proportional(13.0))
}

/// the semibold family at a fixed size
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn set_accent(ctx: &egui::Context, accent: Color32) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals.selection.stroke.color = accent;
        ctx.set_style_of(theme, style);
    }
}

fn system_font() -> Option<Vec<u8>> {
    for path in candidates() {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }
    None
}

fn semibold_font() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    let paths = {
        let root = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        vec![root.join("Fonts").join("seguisb.ttf")]
    };
    #[cfg(target_os = "macos")]
    // sf pro has no separate static semibold file, so use arial bold only as a last resort
    let paths = vec![PathBuf::from(
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    )];
    #[cfg(target_os = "linux")]
    let paths = {
        let mut dirs = vec![
            PathBuf::from("/usr/share/fonts/truetype"),
            PathBuf::from("/usr/share/fonts/opentype"),
            PathBuf::from("/usr/share/fonts/TTF"),
            PathBuf::from("/usr/local/share/fonts"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
        }
        let mut paths = Vec::new();
        for (subdir, name) in [
            ("inter", "Inter-SemiBold.ttf"),
            ("inter", "Inter-SemiBold.otf"),
            ("cantarell", "Cantarell-Bold.otf"),
            ("cantarell", "Cantarell-Bold.ttf"),
            ("noto", "NotoSans-SemiBold.ttf"),
            ("noto", "NotoSans-SemiBold.otf"),
        ] {
            for dir in &dirs {
                paths.push(dir.join(subdir).join(name));
                paths.push(dir.join(name));
            }
        }
        paths
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let paths: Vec<PathBuf> = Vec::new();
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }
    None
}

fn candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        return ["SegUIVar.ttf", "segoeui.ttf"]
            .into_iter()
            .map(|name| root.join("Fonts").join(name))
            .collect();
    }
    #[cfg(target_os = "macos")]
    {
        return vec![PathBuf::from("/System/Library/Fonts/SFNS.ttf")];
    }
    #[cfg(target_os = "linux")]
    {
        let mut dirs = vec![
            PathBuf::from("/usr/share/fonts/truetype"),
            PathBuf::from("/usr/share/fonts/opentype"),
            PathBuf::from("/usr/share/fonts/TTF"),
            PathBuf::from("/usr/local/share/fonts"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/fonts"));
        }
        let names = [
            ("inter", "Inter-Regular.ttf"),
            ("inter", "Inter-Regular.otf"),
            ("inter", "Inter.ttf"),
            ("cantarell", "Cantarell-Regular.otf"),
            ("cantarell", "Cantarell-Regular.ttf"),
            ("noto", "NotoSans-Regular.ttf"),
            ("noto", "NotoSans-Regular.otf"),
            ("dejavu", "DejaVuSans.ttf"),
        ];
        let mut result = Vec::new();
        for (subdir, name) in names {
            for dir in &dirs {
                result.push(dir.join(subdir).join(name));
                result.push(dir.join(name));
            }
        }
        return result;
    }
    #[allow(unreachable_code)]
    Vec::new()
}
