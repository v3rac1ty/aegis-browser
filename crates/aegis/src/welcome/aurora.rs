use std::sync::Arc;

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, TextureHandle, TextureOptions};

use super::projection;

const WIDTH: usize = 64;
const HEIGHT: usize = 40;
const SMALL_WIDTH: usize = 16;
const SMALL_HEIGHT: usize = 10;
fn blob_center(
    left: f32,
    top: f32,
    diameter: f32,
    aspect: f32,
    drift_x: f32,
    drift_y: f32,
) -> (f32, f32) {
    let radius_x = diameter * 0.5;
    let radius_y = radius_x * aspect;
    let margin_y = (radius_y * 0.5).min(0.5);
    let x = ((left + radius_x).clamp(radius_x * 0.5, 1.0 - radius_x * 0.5) + drift_x)
        .clamp(radius_x * 0.5, 1.0 - radius_x * 0.5);
    let y = ((top + radius_y).clamp(margin_y, 1.0 - margin_y) + drift_y)
        .clamp(margin_y, 1.0 - margin_y);
    (x, y)
}

fn fill_image(
    pixels: &mut egui::ColorImage,
    time: f32,
    accent: Color32,
    aspect: f32,
    bloom: [f32; 3],
    spread: f32,
) {
    let blobs = [
        (accent, 0.85, 0.56, -0.14, -0.18, 22.0),
        (
            Color32::from_rgb(0xc2, 0x25, 0x5c),
            0.55,
            0.48,
            1.12,
            -0.06,
            26.0,
        ),
        (
            Color32::from_rgb(0x0c, 0x85, 0x99),
            0.60,
            0.52,
            0.18,
            1.26,
            30.0,
        ),
    ];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let px = (x as f32 + 0.5) / WIDTH as f32;
            let py = (y as f32 + 0.5) / HEIGHT as f32;
            let mut red = 7.0;
            let mut green = 9.0;
            let mut blue = 20.0;
            for (index, &(color, alpha, diameter, left, top, period)) in blobs.iter().enumerate() {
                let phase = time * std::f32::consts::TAU / period;
                let direction = if index == 1 { -1.0 } else { 1.0 };
                let (center_x, center_y) = blob_center(
                    left,
                    top,
                    diameter,
                    aspect,
                    phase.sin() * 0.14 * direction,
                    phase.cos() * 0.12,
                );
                // blobs open out from the middle of the window
                let center_x = 0.5 + (center_x - 0.5) * spread;
                let center_y = 0.5 + (center_y - 0.5) * spread;
                let size =
                    diameter * (0.55 + 0.45 * bloom[index]) * (1.05 + 0.15 * (phase * 0.7).sin());
                let dx = (px - center_x) / (size * 0.5);
                let dy = (py - center_y) / (size * aspect * 0.5);
                let distance = (dx * dx + dy * dy).sqrt();
                let t = (1.0 - distance).clamp(0.0, 1.0);
                let falloff = t * t * (3.0 - 2.0 * t) * alpha * bloom[index];
                red += f32::from(color.r()) * falloff;
                green += f32::from(color.g()) * falloff;
                blue += f32::from(color.b()) * falloff;
            }
            pixels.pixels[y * WIDTH + x] = Color32::from_rgb(
                red.min(255.0) as u8,
                green.min(255.0) as u8,
                blue.min(255.0) as u8,
            );
        }
    }
}

pub struct Aurora {
    pixels: [Arc<egui::ColorImage>; 2],
    small_pixels: [Arc<egui::ColorImage>; 2],
    next_buffer: usize,
    background: TextureHandle,
    frost: TextureHandle,
    rim: Vec<Pos2>,
}

impl Aurora {
    pub fn new(ctx: &egui::Context) -> Self {
        let pixels = std::array::from_fn(|_| {
            Arc::new(egui::ColorImage::new(
                [WIDTH, HEIGHT],
                vec![Color32::from_rgb(7, 9, 20); WIDTH * HEIGHT],
            ))
        });
        let small_pixels = std::array::from_fn(|_| {
            Arc::new(egui::ColorImage::new(
                [SMALL_WIDTH, SMALL_HEIGHT],
                vec![Color32::from_rgb(7, 9, 20); SMALL_WIDTH * SMALL_HEIGHT],
            ))
        });
        let background =
            ctx.load_texture("welcome aurora", pixels[0].clone(), TextureOptions::LINEAR);
        let frost = ctx.load_texture(
            "welcome frost",
            small_pixels[0].clone(),
            TextureOptions::LINEAR,
        );
        Self {
            pixels,
            small_pixels,
            next_buffer: 1,
            background,
            frost,
            rim: projection::rounded_rect_points(
                Rect::from_min_size(Pos2::ZERO, egui::vec2(1.0, 1.0)),
                0.1,
                5,
            ),
        }
    }

    /// strength 0 is the bare base color, 1 the resting aurora
    pub fn update(&mut self, time: f32, accent: Color32, aspect: f32, strength: f32) -> bool {
        let index = self.next_buffer;
        let Some(pixels) = Arc::get_mut(&mut self.pixels[index]) else {
            return false;
        };
        let Some(small_pixels) = Arc::get_mut(&mut self.small_pixels[index]) else {
            return false;
        };
        let strength = strength.clamp(0.0, 1.0);
        // the blobs follow one another a little
        let bloom =
            std::array::from_fn(|index| (strength * 1.3 - index as f32 * 0.15).clamp(0.0, 1.0));
        fill_image(pixels, time, accent, aspect, bloom, strength);
        for y in 0..SMALL_HEIGHT {
            for x in 0..SMALL_WIDTH {
                let mut channels = [0_u16; 3];
                for dy in 0..4 {
                    for dx in 0..4 {
                        let color = pixels.pixels[(y * 4 + dy) * WIDTH + x * 4 + dx];
                        channels[0] += u16::from(color.r());
                        channels[1] += u16::from(color.g());
                        channels[2] += u16::from(color.b());
                    }
                }
                small_pixels.pixels[y * SMALL_WIDTH + x] = Color32::from_rgb(
                    (channels[0] / 16) as u8,
                    (channels[1] / 16) as u8,
                    (channels[2] / 16) as u8,
                );
            }
        }
        self.background
            .set(self.pixels[index].clone(), TextureOptions::LINEAR);
        self.frost
            .set(self.small_pixels[index].clone(), TextureOptions::LINEAR);
        self.next_buffer = 1 - index;
        true
    }

    pub fn paint_background(&self, painter: &egui::Painter, window: Rect) {
        painter.image(
            self.background.id(),
            window,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    pub fn paint_background_clipped(&self, painter: &egui::Painter, rect: Rect, window: Rect) {
        let uv = Rect::from_min_max(
            Pos2::new(
                (rect.left() - window.left()) / window.width(),
                (rect.top() - window.top()) / window.height(),
            ),
            Pos2::new(
                (rect.right() - window.left()) / window.width(),
                (rect.bottom() - window.top()) / window.height(),
            ),
        );
        painter
            .with_clip_rect(rect)
            .image(self.background.id(), rect, uv, Color32::WHITE);
    }

    pub fn paint_frost(&mut self, painter: &egui::Painter, card: Rect, window: Rect) {
        projection::fill_rounded_rect_points(&mut self.rim, card, 34.0, 5);
        let mut mesh = Mesh::with_texture(self.frost.id());
        let center = card.center();
        let uv = |point: Pos2| {
            Pos2::new(
                (point.x - window.left()) / window.width(),
                (point.y - window.top()) / window.height(),
            )
        };
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center,
            uv: uv(center),
            color: Color32::WHITE,
        });
        for point in &self.rim {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: *point,
                uv: uv(*point),
                color: Color32::WHITE,
            });
        }
        for index in 1..self.rim.len() {
            mesh.add_triangle(0, index as u32, index as u32 + 1);
        }
        mesh.add_triangle(0, self.rim.len() as u32, 1);
        painter.add(mesh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resting_aurora_has_all_three_colors() {
        let mut image =
            egui::ColorImage::new([WIDTH, HEIGHT], vec![Color32::TRANSPARENT; WIDTH * HEIGHT]);
        let aspect = 2000.0 / 1067.0;
        fill_image(
            &mut image,
            0.0,
            Color32::from_rgb(90, 70, 220),
            aspect,
            [1.0; 3],
            1.0,
        );
        let sample = |left, top, diameter| {
            let (x, y) = blob_center(left, top, diameter, aspect, 0.0, 0.12);
            image.pixels[(y * HEIGHT as f32) as usize * WIDTH + (x * WIDTH as f32) as usize]
        };
        let indigo = sample(-0.14, -0.18, 0.56);
        let rose = sample(1.12, -0.06, 0.48);
        let teal = sample(0.18, 1.26, 0.52);
        assert!(indigo.b() > indigo.r() && indigo.b() > indigo.g());
        assert!(rose.r() > rose.g() && rose.r() > rose.b());
        assert!(teal.g() > teal.r() && teal.b() > teal.r());
    }
}
