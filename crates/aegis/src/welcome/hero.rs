use std::time::Instant;

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Shape, Stroke};

use super::projection::{self, Vec3};
use crate::motion::Spring;

const REST_RX: f32 = -6.0;
const REST_RY: f32 = 14.0;

fn target_pose(px: f32, py: f32, hovered: bool) -> (f32, f32, f32) {
    if hovered {
        (REST_RX + py * 18.0, REST_RY - px * 24.0, -24.0)
    } else {
        (REST_RX, REST_RY, 0.0)
    }
}

fn vertical_drift(time: f32, reduced: bool) -> f32 {
    if reduced {
        0.0
    } else {
        (time * std::f32::consts::TAU / 6.0).sin() * 8.0
    }
}

pub struct Hero {
    rx: Spring,
    ry: Spring,
    lift: Spring,
    last: Instant,
    rim: Vec<Pos2>,
    projected: Vec<Pos2>,
}

impl Hero {
    pub fn new() -> Self {
        Self {
            rx: Spring::new(REST_RX),
            ry: Spring::new(REST_RY),
            lift: Spring::new(0.0),
            last: Instant::now(),
            rim: Vec::with_capacity(40),
            projected: Vec::with_capacity(40),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        box_rect: Rect,
        accent: Color32,
        time: f32,
        reduced: bool,
        intro: f32,
    ) {
        let hovered = ui
            .interact(
                box_rect,
                ui.scope_id().with("welcome_hero"),
                egui::Sense::hover(),
            )
            .hovered();
        let (px, py) =
            ui.ctx()
                .input(|input| input.pointer.hover_pos())
                .map_or((0.0, 0.0), |pos| {
                    (
                        ((pos.x - box_rect.left()) / box_rect.width() - 0.5).clamp(-0.5, 0.5),
                        ((pos.y - box_rect.top()) / box_rect.height() - 0.5).clamp(-0.5, 0.5),
                    )
                });
        let (rx_target, ry_target, lift_target) = target_pose(px, py, hovered && !reduced);
        self.rx.target = rx_target;
        self.ry.target = ry_target;
        self.lift.target = lift_target;
        let dt = self.last.elapsed().as_secs_f32().min(0.1);
        self.last = Instant::now();
        if reduced {
            self.rx.value = REST_RX;
            self.ry.value = REST_RY;
            self.lift.value = 0.0;
        } else {
            // response = 2π / √120 for stiffness 120 and critical damping
            let response = std::f32::consts::TAU / 120.0_f32.sqrt();
            self.rx.step(dt, response);
            self.ry.step(dt, response);
            self.lift.step(dt, response);
            if !self.rx.settled() || !self.ry.settled() || !self.lift.settled() {
                crate::motion::request_animation_frame(ui.ctx());
            }
        }
        let drift = vertical_drift(time, reduced);
        let center = box_rect.center() + egui::vec2(0.0, -intro * 60.0 + drift);
        let rx = self.rx.value + intro * 22.0;
        let ry = self.ry.value - intro * 48.0;
        let lift = self.lift.value;
        let painter = ui.painter();
        let glow_center = center + egui::vec2(-ry * 0.8, 153.0 + lift * 0.3 + rx * 0.5);
        let mut glow = Mesh::default();
        glow.vertices.push(egui::epaint::Vertex {
            pos: glow_center,
            uv: Pos2::ZERO,
            color: accent.gamma_multiply(0.45),
        });
        for index in 0..=32 {
            let angle = index as f32 * std::f32::consts::TAU / 32.0;
            glow.vertices.push(egui::epaint::Vertex {
                pos: glow_center + egui::vec2(angle.cos() * 175.0, angle.sin() * 47.0),
                uv: Pos2::ZERO,
                color: Color32::TRANSPARENT,
            });
            if index > 0 {
                glow.add_triangle(0, index, index + 1);
            }
        }
        painter.add(glow);
        let frame = Rect::from_center_size(Pos2::ZERO, egui::vec2(360.0, 268.0));
        self.poly(
            painter,
            frame,
            18.0,
            0.0,
            Color32::from_white_alpha(36),
            Stroke::new(1.0, Color32::from_white_alpha(72)),
            rx,
            ry,
            lift,
            center,
        );
        let inside = frame.shrink(7.0);
        let side = Rect::from_min_max(
            inside.min,
            Pos2::new(inside.left() + inside.width() * 0.32, inside.bottom()),
        );
        let page = Rect::from_min_max(Pos2::new(side.right() + 4.0, inside.top()), inside.max);
        self.poly(
            painter,
            side,
            11.0,
            1.0,
            Color32::from_rgb(50, 52, 71),
            Stroke::NONE,
            rx,
            ry,
            lift,
            center,
        );
        self.poly(
            painter,
            page,
            11.0,
            1.0,
            Color32::from_rgb(245, 245, 250),
            Stroke::NONE,
            rx,
            ry,
            lift,
            center,
        );
        for (index, color) in [
            Color32::from_rgb(224, 49, 49),
            Color32::from_rgb(245, 159, 0),
            Color32::from_rgb(43, 138, 62),
        ]
        .into_iter()
        .enumerate()
        {
            let dot = Rect::from_center_size(
                side.min + egui::vec2(18.0 + index as f32 * 13.0, 15.0),
                egui::vec2(7.0, 7.0),
            );
            self.poly(
                painter,
                dot,
                4.0,
                2.0,
                color,
                Stroke::NONE,
                rx,
                ry,
                lift,
                center,
            );
        }
        self.poly(
            painter,
            Rect::from_min_size(
                side.min + egui::vec2(13.0, 37.0),
                egui::vec2(side.width() - 26.0, 19.0),
            ),
            6.0,
            2.0,
            Color32::from_white_alpha(31),
            Stroke::NONE,
            rx,
            ry,
            lift,
            center,
        );
        for index in 0..4 {
            let color = [
                accent,
                Color32::from_rgb(224, 49, 49),
                Color32::from_rgb(43, 138, 62),
                Color32::from_rgb(245, 159, 0),
            ][index];
            let dot = Rect::from_center_size(
                side.min + egui::vec2(22.0 + index as f32 * 23.0, 77.0),
                egui::vec2(15.0, 15.0),
            );
            self.poly(
                painter,
                dot,
                8.0,
                2.0,
                color,
                Stroke::NONE,
                rx,
                ry,
                lift,
                center,
            );
        }
        for index in 0..4 {
            let row = Rect::from_min_size(
                side.min + egui::vec2(10.0, 106.0 + index as f32 * 27.0),
                egui::vec2(side.width() - 20.0, 21.0),
            );
            self.poly(
                painter,
                row,
                5.0,
                2.0,
                if index == 0 {
                    Color32::from_white_alpha(55)
                } else {
                    Color32::from_white_alpha(17)
                },
                Stroke::NONE,
                rx,
                ry,
                lift,
                center,
            );
            if index == 0 {
                self.poly(
                    painter,
                    Rect::from_min_size(row.min, egui::vec2(3.0, 21.0)),
                    1.0,
                    3.0,
                    accent,
                    Stroke::NONE,
                    rx,
                    ry,
                    lift,
                    center,
                );
            }
        }
        self.poly(
            painter,
            Rect::from_min_size(
                side.left_bottom() + egui::vec2(10.0, -33.0),
                egui::vec2(side.width() - 20.0, 23.0),
            ),
            9.0,
            2.0,
            Color32::from_white_alpha(36),
            Stroke::NONE,
            rx,
            ry,
            lift,
            center,
        );
        self.poly(
            painter,
            Rect::from_min_size(page.min + egui::vec2(20.0, 22.0), egui::vec2(51.0, 5.0)),
            2.0,
            2.0,
            accent,
            Stroke::NONE,
            rx,
            ry,
            lift,
            center,
        );
        for (index, width) in [150.0, 123.0].into_iter().enumerate() {
            self.poly(
                painter,
                Rect::from_min_size(
                    page.min + egui::vec2(20.0, 46.0 + index as f32 * 22.0),
                    egui::vec2(width, 12.0),
                ),
                3.0,
                2.0,
                Color32::from_rgb(39, 43, 62),
                Stroke::NONE,
                rx,
                ry,
                lift,
                center,
            );
        }
        for (index, width) in [155.0, 175.0, 135.0].into_iter().enumerate() {
            self.poly(
                painter,
                Rect::from_min_size(
                    page.min + egui::vec2(20.0, 106.0 + index as f32 * 15.0),
                    egui::vec2(width, 5.0),
                ),
                2.0,
                2.0,
                Color32::from_rgb(177, 180, 192),
                Stroke::NONE,
                rx,
                ry,
                lift,
                center,
            );
        }
        let block = Rect::from_min_size(
            page.min + egui::vec2(20.0, 164.0),
            egui::vec2(page.width() - 40.0, 70.0),
        );
        self.poly(
            painter,
            block,
            8.0,
            2.0,
            accent,
            Stroke::NONE,
            rx,
            ry,
            lift,
            center,
        );
        self.poly(
            painter,
            Rect::from_min_size(
                block.center_top(),
                egui::vec2(block.width() / 2.0, block.height()),
            ),
            8.0,
            3.0,
            Color32::from_rgba_unmultiplied(194, 37, 92, 150),
            Stroke::NONE,
            rx,
            ry,
            lift,
            center,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn poly(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        radius: f32,
        z: f32,
        fill: Color32,
        stroke: Stroke,
        rx: f32,
        ry: f32,
        lift: f32,
        center: Pos2,
    ) {
        projection::fill_rounded_rect_points(&mut self.rim, rect, radius, 4);
        self.projected.clear();
        for point in &self.rim {
            self.projected.push(projection::project(
                Vec3 {
                    x: point.x,
                    y: point.y,
                    z,
                },
                rx,
                ry,
                lift,
                1400.0,
                center,
            ));
        }
        painter.add(Shape::convex_polygon(self.projected.clone(), fill, stroke));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corner(x: f32, y: f32, rx: f32, ry: f32, lift: f32) -> Pos2 {
        projection::project(Vec3 { x, y, z: 0.0 }, rx, ry, lift, 1400.0, Pos2::ZERO)
    }

    #[test]
    fn rest_pose_faces_top_left() {
        let top_left = corner(-180.0, -134.0, REST_RX, REST_RY, 0.0);
        let top_right = corner(180.0, -134.0, REST_RX, REST_RY, 0.0);
        let bottom_left = corner(-180.0, 134.0, REST_RX, REST_RY, 0.0);
        let bottom_right = corner(180.0, 134.0, REST_RX, REST_RY, 0.0);
        assert!(top_right.distance(bottom_right) > top_left.distance(bottom_left));
        assert!(bottom_left.distance(bottom_right) > top_left.distance(top_right));
    }

    #[test]
    fn pressing_right_edge_moves_it_away() {
        let (rx, ry, lift) = target_pose(0.5, 0.0, true);
        let rest_top = corner(180.0, -134.0, REST_RX, REST_RY, 0.0);
        let rest_bottom = corner(180.0, 134.0, REST_RX, REST_RY, 0.0);
        let pressed_top = corner(180.0, -134.0, rx, ry, lift);
        let pressed_bottom = corner(180.0, 134.0, rx, ry, lift);
        assert!(pressed_top.distance(pressed_bottom) < rest_top.distance(rest_bottom));
        assert_eq!(lift, -24.0);
    }

    #[test]
    fn float_continues_every_six_seconds() {
        assert!((vertical_drift(1.5, false) - 8.0).abs() < 0.001);
        assert!((vertical_drift(4.5, false) + 8.0).abs() < 0.001);
        assert!((vertical_drift(7.5, false) - 8.0).abs() < 0.001);
        assert_eq!(vertical_drift(1.5, true), 0.0);
        assert_eq!(target_pose(0.0, 0.0, false), (REST_RX, REST_RY, 0.0));
    }
}
