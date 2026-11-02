use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;

const SHOT_DELAY: Duration = Duration::from_millis(2500);

pub fn parse_size(value: &str) -> Option<[f32; 2]> {
    let (width, height) = value.split_once('x')?;
    let width = width.parse::<u32>().ok()?;
    let height = height.parse::<u32>().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some([width as f32, height as f32])
}

/// a plain count of seconds, as in AEGIS_DEBUG_SHOT_AT=1.8
pub fn parse_seconds(value: &str) -> Option<f32> {
    let seconds = value.trim().parse::<f32>().ok()?;
    if !seconds.is_finite() || !(0.0..=3600.0).contains(&seconds) {
        return None;
    }
    Some(seconds)
}

pub struct DebugShot {
    path: PathBuf,
    delay: Duration,
    first_frame: Option<Instant>,
    requested: bool,
    memory_due: Option<Instant>,
    memory_reported: bool,
}

impl DebugShot {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("AEGIS_DEBUG_SHOT")?;
        // an invalid AEGIS_DEBUG_SHOT_AT keeps the default delay
        let delay = std::env::var("AEGIS_DEBUG_SHOT_AT")
            .ok()
            .and_then(|value| parse_seconds(&value))
            .map_or(SHOT_DELAY, Duration::from_secs_f32);
        Some(Self {
            path: PathBuf::from(path),
            delay,
            first_frame: None,
            requested: false,
            memory_due: None,
            memory_reported: false,
        })
    }

    pub fn welcome_finished(&mut self, ctx: &egui::Context) {
        let due = Instant::now() + Duration::from_secs(2);
        self.memory_due = Some(due);
        self.memory_reported = false;
        ctx.request_repaint_after(Duration::from_secs(2));
    }

    pub fn tick(&mut self, ctx: &egui::Context, welcome_show: bool) {
        let first = *self.first_frame.get_or_insert_with(Instant::now);
        if !welcome_show && !self.memory_reported {
            let due = *self
                .memory_due
                .get_or_insert(first + Duration::from_secs(2));
            let remaining = due.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let [width, height] = ctx.fonts(|fonts| fonts.font_image_size());
                let textures = ctx.tex_manager().read().num_allocated();
                eprintln!("debug memory: font_atlas={width}x{height} textures={textures}");
                self.memory_reported = true;
            } else {
                ctx.request_repaint_after(remaining);
            }
        }
        if self.requested {
            return;
        }
        let remaining = self.delay.saturating_sub(first.elapsed());
        if !remaining.is_zero() {
            ctx.request_repaint_after(remaining);
            return;
        }
        self.requested = true;
        let path = self.path.clone();
        let closer = ctx.clone();
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
                if let Err(error) = save_bmp(&path, &image) {
                    eprintln!("debug screenshot: {error}");
                }
                closer.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            })),
        );
    }
}

fn save_bmp(path: &Path, image: &egui::ColorImage) -> io::Result<()> {
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(format!(".tmp-{}", std::process::id()));
    let temporary = PathBuf::from(temporary);
    let result = (|| {
        let mut file = std::fs::File::create(&temporary)?;
        write_bmp(&mut file, image)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn write_bmp(output: &mut impl Write, image: &egui::ColorImage) -> io::Result<()> {
    let [width, height] = image.size;
    let width = i32::try_from(width).map_err(io::Error::other)?;
    let height = i32::try_from(height).map_err(io::Error::other)?;
    let pixel_bytes = image
        .pixels
        .len()
        .checked_mul(4)
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| io::Error::other("screenshot is too large"))?;
    if width == 0 || height == 0 || image.pixels.len() != width as usize * height as usize {
        return Err(io::Error::other("invalid screenshot dimensions"));
    }
    let file_bytes = 54_u32
        .checked_add(pixel_bytes)
        .ok_or_else(|| io::Error::other("screenshot is too large"))?;

    output.write_all(b"BM")?;
    output.write_all(&file_bytes.to_le_bytes())?;
    output.write_all(&[0; 4])?;
    output.write_all(&54_u32.to_le_bytes())?;
    output.write_all(&40_u32.to_le_bytes())?;
    output.write_all(&width.to_le_bytes())?;
    output.write_all(&(-height).to_le_bytes())?;
    output.write_all(&1_u16.to_le_bytes())?;
    output.write_all(&32_u16.to_le_bytes())?;
    output.write_all(&0_u32.to_le_bytes())?;
    output.write_all(&pixel_bytes.to_le_bytes())?;
    output.write_all(&[0; 16])?;
    for pixel in &image.pixels {
        output.write_all(&[pixel.b(), pixel.g(), pixel.r(), pixel.a()])?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn size_parser() {
        assert_eq!(parse_size("1280x800"), Some([1280.0, 800.0]));
        assert_eq!(parse_size("1280"), None);
        assert_eq!(parse_size("12xno"), None);
        assert_eq!(parse_size("0x800"), None);
        assert_eq!(parse_size("1280x0"), None);
    }

    #[test]
    fn seconds_parser() {
        assert_eq!(parse_seconds("1.8"), Some(1.8));
        assert_eq!(parse_seconds(" 0 "), Some(0.0));
        assert_eq!(parse_seconds("-1"), None);
        assert_eq!(parse_seconds("nan"), None);
        assert_eq!(parse_seconds("inf"), None);
        assert_eq!(parse_seconds("soon"), None);
        assert_eq!(parse_seconds("100000"), None);
    }

    #[test]
    fn bmp_header_and_pixel_order() {
        let image = egui::ColorImage::new(
            [2, 2],
            vec![
                egui::Color32::from_rgba_premultiplied(1, 2, 3, 4),
                egui::Color32::from_rgba_premultiplied(5, 6, 7, 8),
                egui::Color32::from_rgba_premultiplied(9, 10, 11, 12),
                egui::Color32::from_rgba_premultiplied(13, 14, 15, 16),
            ],
        );
        let mut bytes = Vec::new();
        assert!(write_bmp(&mut bytes, &image).is_ok());
        assert_eq!(&bytes[0..2], b"BM");
        assert_eq!(u32::from_le_bytes(bytes[2..6].try_into().unwrap()), 70);
        assert_eq!(u32::from_le_bytes(bytes[10..14].try_into().unwrap()), 54);
        assert_eq!(u32::from_le_bytes(bytes[14..18].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(bytes[18..22].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(bytes[22..26].try_into().unwrap()), -2);
        assert_eq!(u16::from_le_bytes(bytes[26..28].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 32);
        assert_eq!(u32::from_le_bytes(bytes[34..38].try_into().unwrap()), 16);
        assert_eq!(
            &bytes[54..],
            &[3, 2, 1, 4, 7, 6, 5, 8, 11, 10, 9, 12, 15, 14, 13, 16]
        );
    }
}
