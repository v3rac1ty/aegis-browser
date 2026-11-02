use aegis_core::{Glass, GroupColor, Settings};
use eframe::egui::{self, Color32, Mesh, Rect, TextureHandle, TextureOptions, TextureWrapMode};

use crate::theme::{self, Theme};

const SIZE: usize = 128;

pub fn noise_values(seed: u32) -> Vec<u8> {
    let mut state = seed;
    let mut values = Vec::with_capacity(SIZE * SIZE);
    for _ in 0..SIZE * SIZE {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        values.push((state >> 24) as u8);
    }
    values
}

pub fn load_noise(ctx: &egui::Context) -> TextureHandle {
    let pixels = noise_values(0xAE615)
        .into_iter()
        .map(|value| {
            let white = value >= 128;
            let color = if white { 255 } else { 0 };
            let alpha = value.abs_diff(128).saturating_mul(2);
            Color32::from_rgba_unmultiplied(color, color, color, alpha)
        })
        .collect();
    ctx.load_texture(
        "window grain",
        egui::ColorImage::new([SIZE, SIZE], pixels),
        TextureOptions {
            wrap_mode: TextureWrapMode::Repeat,
            ..TextureOptions::NEAREST
        },
    )
}

pub fn paint(
    ui: &egui::Ui,
    settings: &Settings,
    theme: Theme,
    space: GroupColor,
    glass_supported: bool,
    texture: &TextureHandle,
) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let solid = !glass_supported || settings.glass == Glass::Solid;
    if solid {
        painter.rect_filled(rect, 0.0, theme.palette.glass_tint_solid);
    } else if settings.glass == Glass::Clear {
        painter.rect_filled(rect, 0.0, theme.palette.glass_tint_clear);
    }
    paint_gradient(painter, rect, settings, theme, space, glass_supported);
    paint_noise(
        painter,
        rect,
        settings.noise,
        theme.dark,
        texture,
        ui.ctx().pixels_per_point(),
    );
}

pub fn paint_gradient(
    painter: &egui::Painter,
    rect: Rect,
    settings: &Settings,
    theme: Theme,
    space: GroupColor,
    glass_supported: bool,
) {
    let solid = !glass_supported || settings.glass == Glass::Solid;
    if settings.glass != Glass::Clear || !glass_supported {
        let alpha = if theme.dark { 0.18 } else { 0.22 };
        let alpha = if solid { alpha * 0.5 } else { alpha };
        let color = |color: Color32| {
            let [r, g, b, _] = color.to_array();
            Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0) as u8)
        };
        let from = color(theme::group_color(space));
        let to = color(theme.accent);
        let mid = Color32::from_rgba_unmultiplied(
            ((u16::from(from.r()) + u16::from(to.r())) / 2) as u8,
            ((u16::from(from.g()) + u16::from(to.g())) / 2) as u8,
            ((u16::from(from.b()) + u16::from(to.b())) / 2) as u8,
            from.a(),
        );
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(rect, Color32::WHITE);
        for (vertex, color) in mesh.vertices.iter_mut().zip([from, mid, mid, to]) {
            vertex.color = color;
        }
        painter.add(mesh);
    }
}

pub fn paint_noise(
    painter: &egui::Painter,
    rect: Rect,
    noise: u8,
    dark: bool,
    texture: &TextureHandle,
    pixels_per_point: f32,
) {
    if noise == 0 {
        return;
    }
    let strength = f32::from(noise) / 100.0 * if dark { 0.14 } else { 0.10 };
    let uv = Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::pos2(
            rect.width() * pixels_per_point / SIZE as f32,
            rect.height() * pixels_per_point / SIZE as f32,
        ),
    );
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(
        rect,
        uv,
        Color32::from_white_alpha((strength * 255.0) as u8),
    );
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_repeatable_and_in_range() {
        let values = noise_values(12);
        assert_eq!(values.len(), 128 * 128);
        assert_eq!(values, noise_values(12));
        assert_ne!(values, noise_values(13));
        assert!(values.iter().any(|&value| value < 64));
        assert!(values.iter().any(|&value| value > 192));
    }
}
