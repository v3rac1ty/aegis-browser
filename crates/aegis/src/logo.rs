// windows executable and macos/linux bundle icons belong to m12 packaging

use std::sync::{Arc, OnceLock};

use eframe::egui::{self, Color32, IconData, Painter, Pos2, Rect};

/// the bright cells of the aegis mark
pub const LOGO: [[bool; 5]; 5] = [
    [false, false, true, false, false],
    [false, true, false, true, false],
    [false, true, true, true, false],
    [true, false, false, false, true],
    [true, false, false, false, true],
];

const GRID_SIZE: f32 = 80.0;
const GAP: f32 = 6.0;
const DOT: f32 = (GRID_SIZE - 4.0 * GAP) / 5.0;
const STEP: f32 = DOT + GAP;

fn dot_centers(origin: Pos2, scale: f32) -> [Pos2; 10] {
    let mut centers = [Pos2::ZERO; 10];
    let mut count = 0;
    for (row, cells) in LOGO.iter().enumerate() {
        for (col, bright) in cells.iter().enumerate() {
            if *bright {
                centers[count] = origin
                    + egui::vec2(
                        (DOT / 2.0 + col as f32 * STEP) * scale,
                        (DOT / 2.0 + row as f32 * STEP) * scale,
                    );
                count += 1;
            }
        }
    }
    centers
}

/// dot centers and the dot radius for a mark drawn in rect, apex dot first
pub fn layout(rect: Rect) -> ([Pos2; 10], f32) {
    let scale = rect.width().min(rect.height()) / GRID_SIZE;
    let origin = rect.center() - egui::vec2(GRID_SIZE * scale / 2.0, GRID_SIZE * scale / 2.0);
    (dot_centers(origin, scale), DOT * scale / 2.0)
}

pub fn paint(painter: &Painter, rect: Rect, color: Color32) {
    let (centers, radius) = layout(rect);
    for center in centers {
        painter.circle_filled(center, radius, color);
    }
}

pub fn window_icon(size: u32) -> IconData {
    let side = size as f32;
    let half = side / 2.0;
    let corner = side * 0.22;
    let grid_scale = side * 0.62 / GRID_SIZE;
    let grid_start = (side - side * 0.62) / 2.0;
    let centers = dot_centers(Pos2::new(grid_start, grid_start), grid_scale);
    let mut rgba = vec![0; size as usize * size as usize * 4];

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let qx = (px - half).abs() - (half - corner);
            let qy = (py - half).abs() - (half - corner);
            let outside = qx.max(0.0).hypot(qy.max(0.0));
            let inside = qx.max(qy).min(0.0);
            let tile_coverage = (0.5 - (outside + inside - corner)).clamp(0.0, 1.0);
            if tile_coverage == 0.0 {
                continue;
            }

            let mut dot_opacity = 0.0_f32;
            for center in centers {
                let distance = (px - center.x).hypot(py - center.y) - DOT * grid_scale / 2.0;
                let coverage = (0.5 - distance).clamp(0.0, 1.0);
                dot_opacity = dot_opacity.max(coverage);
            }

            let index = (y as usize * size as usize + x as usize) * 4;
            for channel in 0..3 {
                let tile = [0x14, 0x16, 0x1f][channel] as f32;
                rgba[index + channel] = (tile + (255.0 - tile) * dot_opacity).round() as u8;
            }
            rgba[index + 3] = (255.0 * tile_coverage).round() as u8;
        }
    }

    IconData {
        rgba,
        width: size,
        height: size,
    }
}

pub fn icon() -> Arc<IconData> {
    static ICON: OnceLock<Arc<IconData>> = OnceLock::new();
    Arc::clone(ICON.get_or_init(|| Arc::new(window_icon(128))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(icon: &IconData, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * icon.width + x) * 4) as usize;
        [
            icon.rgba[i],
            icon.rgba[i + 1],
            icon.rgba[i + 2],
            icon.rgba[i + 3],
        ]
    }

    #[test]
    fn icon_pixels_and_pattern() {
        let icon = window_icon(64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
        assert_eq!(pixel(&icon, 0, 0)[3], 0);
        assert_eq!(pixel(&icon, 32, 32)[3], 255);

        let bright = pixel(&icon, 32, 15);
        let empty = pixel(&icon, 15, 15);
        let tile = pixel(&icon, 32, 5);
        assert!(bright[0] > 235, "bright dot: {bright:?}");
        assert_eq!(empty, [0x14, 0x16, 0x1f, 255]);
        assert_eq!(tile, empty);
        assert_eq!(dot_centers(Pos2::ZERO, 1.0).len(), 10);
    }
}
