use eframe::egui::{Pos2, Rect};

#[derive(Clone, Copy, Debug)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub fn project(
    point: Vec3,
    rx_deg: f32,
    ry_deg: f32,
    lift: f32,
    perspective: f32,
    center: Pos2,
) -> Pos2 {
    let rx = rx_deg.to_radians();
    let ry = ry_deg.to_radians();
    let y = point.y * rx.cos() - point.z * rx.sin();
    let z = point.y * rx.sin() + point.z * rx.cos();
    let x = point.x * ry.cos() + z * ry.sin();
    let z = -point.x * ry.sin() + z * ry.cos() - lift;
    let scale = perspective / (perspective + z).max(1.0);
    Pos2::new(center.x + x * scale, center.y + y * scale)
}

pub fn rounded_rect_points(rect: Rect, radius: f32, segments: usize) -> Vec<Pos2> {
    let mut points = Vec::with_capacity(4 * (segments + 1));
    fill_rounded_rect_points(&mut points, rect, radius, segments);
    points
}

pub fn fill_rounded_rect_points(points: &mut Vec<Pos2>, rect: Rect, radius: f32, segments: usize) {
    points.clear();
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    for (center, start) in [
        (
            Pos2::new(rect.right() - r, rect.top() + r),
            -std::f32::consts::FRAC_PI_2,
        ),
        (Pos2::new(rect.right() - r, rect.bottom() - r), 0.0),
        (
            Pos2::new(rect.left() + r, rect.bottom() - r),
            std::f32::consts::FRAC_PI_2,
        ),
        (
            Pos2::new(rect.left() + r, rect.top() + r),
            std::f32::consts::PI,
        ),
    ] {
        for step in 0..=segments {
            let angle = start + std::f32::consts::FRAC_PI_2 * step as f32 / segments.max(1) as f32;
            points.push(Pos2::new(
                center.x + r * angle.cos(),
                center.y + r * angle.sin(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_at_zero_tilt_and_lift() {
        let center = Pos2::new(100.0, 200.0);
        assert_eq!(
            project(
                Vec3 {
                    x: 12.0,
                    y: -8.0,
                    z: 0.0
                },
                0.0,
                0.0,
                0.0,
                1400.0,
                center
            ),
            Pos2::new(112.0, 192.0)
        );
    }

    #[test]
    fn opposite_points_are_symmetric() {
        let center = Pos2::new(100.0, 200.0);
        let a = project(
            Vec3 {
                x: 40.0,
                y: 20.0,
                z: 0.0,
            },
            0.0,
            0.0,
            0.0,
            1400.0,
            center,
        );
        let b = project(
            Vec3 {
                x: -40.0,
                y: -20.0,
                z: 0.0,
            },
            0.0,
            0.0,
            0.0,
            1400.0,
            center,
        );
        assert!((a.x + b.x - 2.0 * center.x).abs() < 0.001);
        assert!((a.y + b.y - 2.0 * center.y).abs() < 0.001);
    }

    #[test]
    fn distant_points_move_toward_center() {
        let center = Pos2::ZERO;
        let near = project(
            Vec3 {
                x: 100.0,
                y: 0.0,
                z: 0.0,
            },
            0.0,
            0.0,
            0.0,
            1400.0,
            center,
        );
        let far = project(
            Vec3 {
                x: 100.0,
                y: 0.0,
                z: 300.0,
            },
            0.0,
            0.0,
            0.0,
            1400.0,
            center,
        );
        assert!(far.x < near.x);
    }
}
