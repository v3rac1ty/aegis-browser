use std::fmt::Write;
use std::sync::Arc;

use aegis_core::{Accent, MotionPref, Settings};
use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Vec2};

use crate::motion::Motion;

pub struct AccentPicker {
    pub open: bool,
    mesh: Arc<Mesh>,
    mesh_center: Pos2,
    mesh_value: f32,
    gradient: Arc<Mesh>,
    gradient_rect: Rect,
    gradient_hs: (f32, f32),
    hex_color: [u8; 3],
    hex: String,
    motion: Motion,
}

impl Default for AccentPicker {
    fn default() -> Self {
        Self {
            open: false,
            mesh: Arc::new(Mesh::default()),
            mesh_center: Pos2::ZERO,
            mesh_value: -1.0,
            gradient: Arc::new(Mesh::default()),
            gradient_rect: Rect::NOTHING,
            gradient_hs: (-1.0, -1.0),
            hex_color: [0; 3],
            hex: String::with_capacity(7),
            motion: Motion::new(),
        }
    }
}

pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let h = hue.rem_euclid(1.0) * 6.0;
    let c = value * saturation.clamp(0.0, 1.0);
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    [
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    ]
}

pub fn rgb_to_hsv(rgb: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = rgb.map(|channel| f32::from(channel) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta < f32::EPSILON {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    (hue, if max == 0.0 { 0.0 } else { delta / max }, max)
}

pub fn wheel_position(center: Pos2, radius: f32, hue: f32, saturation: f32) -> Pos2 {
    let angle = hue * std::f32::consts::TAU;
    center + Vec2::angled(angle) * radius * saturation.clamp(0.0, 1.0)
}

pub fn wheel_hs(center: Pos2, radius: f32, position: Pos2) -> (f32, f32) {
    let offset = position - center;
    (
        offset.angle().rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU,
        (offset.length() / radius).clamp(0.0, 1.0),
    )
}

impl AccentPicker {
    #[cfg(test)]
    pub(crate) fn mesh_ready(&self) -> bool {
        !self.mesh.vertices.is_empty()
    }

    fn rebuild(&mut self, center: Pos2, value: f32) {
        if self.mesh_center == center && self.mesh_value == value {
            return;
        }
        if self.mesh_value == value {
            let offset = center - self.mesh_center;
            for vertex in &mut Arc::make_mut(&mut self.mesh).vertices {
                vertex.pos += offset;
            }
            self.mesh_center = center;
            return;
        }
        self.mesh_center = center;
        self.mesh_value = value;
        let mesh = Arc::make_mut(&mut self.mesh);
        mesh.vertices.clear();
        mesh.indices.clear();
        let slices = 96;
        let grey = (value * 255.0).round() as u8;
        mesh.colored_vertex(center, Color32::from_rgb(grey, grey, grey));
        for slice in 0..=slices {
            let hue = slice as f32 / slices as f32;
            let [r, g, b] = hsv_to_rgb(hue, 1.0, value);
            mesh.colored_vertex(
                wheel_position(center, 74.0, hue, 1.0),
                Color32::from_rgb(r, g, b),
            );
        }
        for slice in 0..slices {
            mesh.indices.extend_from_slice(&[0, slice + 1, slice + 2]);
        }
    }

    fn rebuild_gradient(&mut self, track: Rect, hue: f32, saturation: f32) {
        if self.gradient_rect == track && self.gradient_hs == (hue, saturation) {
            return;
        }
        self.gradient_rect = track;
        self.gradient_hs = (hue, saturation);
        let mesh = Arc::make_mut(&mut self.gradient);
        mesh.vertices.clear();
        mesh.indices.clear();
        for (index, brightness) in [0.35, 1.0].into_iter().enumerate() {
            let [r, g, b] = hsv_to_rgb(hue, saturation, brightness);
            let x = if index == 0 {
                track.left()
            } else {
                track.right()
            };
            mesh.colored_vertex(egui::pos2(x, track.top()), Color32::from_rgb(r, g, b));
            mesh.colored_vertex(egui::pos2(x, track.bottom()), Color32::from_rgb(r, g, b));
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
    }

    /// swatches, then the wheel under them while it is open
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Settings,
        size: f32,
        preference: MotionPref,
    ) {
        ui.horizontal(|ui| {
            self.swatches(ui, settings, size, Color32::TRANSPARENT, Color32::WHITE);
        });
        self.wheel(ui, settings, preference);
    }

    /// one strip of swatches, left to right, then the custom rainbow
    pub fn swatches(
        &mut self,
        ui: &mut egui::Ui,
        settings: &mut Settings,
        size: f32,
        gap: Color32,
        ring: Color32,
    ) {
        let count = Accent::ALL.len() + 1;
        let (strip, _) =
            ui.allocate_exact_size(egui::vec2(strip_width(size, count), size), Sense::hover());
        let radius = dot_radius(size);
        for (index, accent) in Accent::ALL.into_iter().enumerate() {
            let cell = swatch_cell(strip, size, index);
            let response = ui.interact(
                cell,
                ui.scope_id().with(("accent_swatch", index)),
                Sense::click(),
            );
            let [r, g, b] = accent.rgb();
            ui.painter()
                .circle_filled(cell.center(), radius, Color32::from_rgb(r, g, b));
            if settings.accent == accent {
                paint_ring(ui, cell.center(), radius, gap, ring);
            }
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, accent.label())
            });
            if response.clicked() {
                settings.accent = accent;
                self.open = false;
            }
        }
        let cell = swatch_cell(strip, size, count - 1);
        let response = ui.interact(cell, ui.scope_id().with("accent_custom"), Sense::click());
        let width = radius * 0.46;
        for index in 0..12 {
            let hue = index as f32 / 12.0;
            let [r, g, b] = hsv_to_rgb(hue, 1.0, 1.0);
            let ring_radius = radius - width * 0.5;
            let from = wheel_position(cell.center(), ring_radius, hue, 1.0);
            let to = wheel_position(cell.center(), ring_radius, (index + 1) as f32 / 12.0, 1.0);
            ui.painter()
                .line_segment([from, to], Stroke::new(width, Color32::from_rgb(r, g, b)));
        }
        if settings.accent == Accent::Custom {
            paint_ring(ui, cell.center(), radius, gap, ring);
        }
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Custom accent"));
        if response.clicked() {
            self.open = !self.open;
        }
    }

    /// the color wheel and brightness track, drawn only while open
    pub fn wheel(&mut self, ui: &mut egui::Ui, settings: &mut Settings, preference: MotionPref) {
        if !self.open {
            return;
        }
        ui.push_id("accent_wheel", |ui| {
            let (mut hue, mut saturation, mut value) = rgb_to_hsv(settings.accent_rgb());
            value = value.clamp(0.35, 1.0);
            let (wheel, response) =
                ui.allocate_exact_size(egui::vec2(148.0, 148.0), Sense::click_and_drag());
            let center = wheel.center();
            self.rebuild(center, value);
            ui.painter().add(Shape::Mesh(Arc::clone(&self.mesh)));
            if (response.dragged() || response.clicked())
                && let Some(position) = response.interact_pointer_pos()
            {
                (hue, saturation) = wheel_hs(center, 74.0, position);
                settings.custom_accent = hsv_to_rgb(hue, saturation, value);
                settings.accent = Accent::Custom;
            }
            let target = wheel_position(center, 74.0, hue, saturation);
            let dt = self.motion.frame();
            let marker_x = self.motion.value(
                ui.ctx(),
                ui.scope_id().with("marker_x"),
                target.x,
                0.24,
                dt,
                preference,
            );
            let marker_y = self.motion.value(
                ui.ctx(),
                ui.scope_id().with("marker_y"),
                target.y,
                0.24,
                dt,
                preference,
            );
            let marker = egui::pos2(marker_x, marker_y);
            ui.painter()
                .circle_stroke(marker, 6.0, Stroke::new(2.0, Color32::WHITE));
            ui.painter()
                .circle_stroke(marker, 7.5, Stroke::new(1.0, Color32::BLACK));
            ui.add_space(8.0);
            let (track, response) =
                ui.allocate_exact_size(egui::vec2(148.0, 20.0), Sense::click_and_drag());
            let track = Rect::from_center_size(track.center(), egui::vec2(148.0, 10.0));
            self.rebuild_gradient(track, hue, saturation);
            ui.painter().add(Shape::Mesh(Arc::clone(&self.gradient)));
            if (response.dragged() || response.clicked())
                && let Some(position) = response.interact_pointer_pos()
            {
                value = (0.35
                    + 0.65 * ((position.x - track.left()) / track.width()).clamp(0.0, 1.0))
                .clamp(0.35, 1.0);
                settings.custom_accent = hsv_to_rgb(hue, saturation, value);
                settings.accent = Accent::Custom;
            }
            let x = track.left() + track.width() * ((value - 0.35) / 0.65);
            ui.painter()
                .circle_filled(egui::pos2(x, track.center().y), 6.0, Color32::WHITE);
            let color = settings.accent_rgb();
            if self.hex_color != color || self.hex.is_empty() {
                self.hex_color = color;
                self.hex.clear();
                let [r, g, b] = color;
                let _ = write!(self.hex, "#{r:02X}{g:02X}{b:02X}");
            }
            ui.label(&self.hex);
        });
    }
}

const SWATCH_GAP: f32 = 4.0;

fn dot_radius(size: f32) -> f32 {
    size * 0.5 - 4.0
}

pub(crate) fn strip_width(size: f32, count: usize) -> f32 {
    count as f32 * size + count.saturating_sub(1) as f32 * SWATCH_GAP
}

// cells run left to right in list order
fn swatch_cell(strip: Rect, size: f32, index: usize) -> Rect {
    Rect::from_min_size(
        egui::pos2(
            strip.left() + index as f32 * (size + SWATCH_GAP),
            strip.top(),
        ),
        egui::vec2(size, size),
    )
}

// 2 pt gap in the surface color, then a 2 pt ring
fn paint_ring(ui: &egui::Ui, center: Pos2, radius: f32, gap: Color32, ring: Color32) {
    ui.painter()
        .circle_stroke(center, radius + 1.0, Stroke::new(2.0, gap));
    ui.painter()
        .circle_stroke(center, radius + 3.0, Stroke::new(2.0, ring));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_and_wheel_round_trip() {
        for color in [[59, 91, 219], [12, 133, 153], [255, 0, 0], [128, 128, 128]] {
            let (h, s, v) = rgb_to_hsv(color);
            let result = hsv_to_rgb(h, s, v);
            assert!(result.iter().zip(color).all(|(a, b)| a.abs_diff(b) <= 1));
        }
        let center = egui::pos2(80.0, 80.0);
        for (hue, saturation) in [(0.0, 0.8), (0.25, 1.0), (0.75, 0.5)] {
            let (h, s) = wheel_hs(center, 74.0, wheel_position(center, 74.0, hue, saturation));
            assert!((h - hue).abs() < 0.0001);
            assert!((s - saturation).abs() < 0.0001);
        }
        assert_eq!(
            wheel_hs(center, 74.0, center + egui::vec2(200.0, 0.0)).1,
            1.0
        );
    }

    #[test]
    fn swatches_run_left_to_right_in_list_order() {
        let strip = Rect::from_min_size(egui::pos2(100.0, 10.0), egui::vec2(252.0, 28.0));
        let mut last = f32::MIN;
        for index in 0..=Accent::ALL.len() {
            let cell = swatch_cell(strip, 28.0, index);
            assert!(cell.left() > last);
            assert!(cell.right() <= strip.right() + 0.1);
            last = cell.left();
        }
        assert_eq!(Accent::ALL[0], Accent::Indigo);
        assert_eq!(strip_width(28.0, 8), 28.0 * 8.0 + 4.0 * 7.0);
        // gap 2 and ring 2 fit inside the cell
        assert!(dot_radius(28.0) + 4.0 <= 14.0);
    }
}
