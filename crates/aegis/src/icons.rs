use eframe::egui::{self, Color32, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};

pub type Icon = fn(&Painter, Rect, Color32);

fn point(rect: Rect, x: f32, y: f32) -> Pos2 {
    rect.min + Vec2::new(rect.width() * x / 24.0, rect.height() * y / 24.0)
}

fn stroke(rect: Rect, color: Color32) -> Stroke {
    Stroke::new(1.75 * rect.width() / 16.0, color)
}

fn path(p: &Painter, rect: Rect, color: Color32, points: &[(f32, f32)]) {
    let points: Vec<_> = points.iter().map(|&(x, y)| point(rect, x, y)).collect();
    let width = stroke(rect, color).width;
    p.add(Shape::line(points.clone(), Stroke::new(width, color)));
    for point in points {
        p.circle_filled(point, width / 2.0, color);
    }
}

fn stroke_points(p: &Painter, rect: Rect, color: Color32, points: &[(f32, f32)]) {
    for pair in points.windows(2) {
        p.line_segment(
            [
                point(rect, pair[0].0, pair[0].1),
                point(rect, pair[1].0, pair[1].1),
            ],
            stroke(rect, color),
        );
    }
}

pub fn sparkle(p: &Painter, r: Rect, c: Color32) {
    stroke_points(
        p,
        r,
        c,
        &[
            (10.5, 2.0),
            (11.2, 5.4),
            (12.1, 7.5),
            (13.6, 9.0),
            (17.0, 10.5),
            (13.6, 12.0),
            (12.1, 13.5),
            (11.2, 15.6),
            (10.5, 19.0),
            (9.8, 15.6),
            (8.9, 13.5),
            (7.4, 12.0),
            (4.0, 10.5),
            (7.4, 9.0),
            (8.9, 7.5),
            (9.8, 5.4),
            (10.5, 2.0),
        ],
    );
    stroke_points(
        p,
        r,
        c,
        &[
            (19.0, 2.0),
            (19.5, 4.1),
            (22.0, 5.0),
            (19.5, 5.9),
            (19.0, 8.0),
            (18.5, 5.9),
            (16.0, 5.0),
            (18.5, 4.1),
            (19.0, 2.0),
        ],
    );
}

pub fn feather(p: &Painter, r: Rect, c: Color32) {
    stroke_points(
        p,
        r,
        c,
        &[
            (5.0, 19.0),
            (5.7, 14.0),
            (7.2, 9.8),
            (10.0, 6.0),
            (13.7, 3.7),
            (18.8, 2.6),
            (21.0, 3.0),
            (20.4, 6.8),
            (18.5, 10.6),
            (15.5, 13.5),
            (11.0, 15.4),
            (7.5, 16.0),
        ],
    );
    stroke_points(
        p,
        r,
        c,
        &[
            (3.0, 22.0),
            (6.5, 18.1),
            (10.3, 14.0),
            (14.0, 10.1),
            (17.5, 6.5),
            (20.4, 3.5),
        ],
    );
}

pub fn chevron_left(p: &Painter, r: Rect, c: Color32) {
    path(p, r, c, &[(14.5, 5.0), (7.5, 12.0), (14.5, 19.0)]);
}
pub fn chevron_right(p: &Painter, r: Rect, c: Color32) {
    path(p, r, c, &[(9.5, 5.0), (16.5, 12.0), (9.5, 19.0)]);
}
pub fn chevron_down(p: &Painter, r: Rect, c: Color32) {
    path(p, r, c, &[(5.0, 9.0), (12.0, 16.0), (19.0, 9.0)]);
}
pub fn plus(p: &Painter, r: Rect, c: Color32) {
    path(p, r, c, &[(12.0, 5.0), (12.0, 19.0)]);
    path(p, r, c, &[(5.0, 12.0), (19.0, 12.0)]);
}
pub fn close(p: &Painter, r: Rect, c: Color32) {
    path(p, r, c, &[(6.0, 6.0), (18.0, 18.0)]);
    path(p, r, c, &[(18.0, 6.0), (6.0, 18.0)]);
}
pub fn check(p: &Painter, r: Rect, c: Color32) {
    path(p, r, c, &[(4.0, 12.0), (10.0, 18.0), (20.0, 6.0)]);
}
pub fn dash(p: &Painter, r: Rect, c: Color32) {
    path(p, r, c, &[(5.0, 12.0), (19.0, 12.0)]);
}
pub fn drag_handle(p: &Painter, r: Rect, c: Color32) {
    for x in [8.0, 16.0] {
        for y in [5.0, 12.0, 19.0] {
            p.circle_filled(point(r, x, y), 0.75, c);
        }
    }
}
pub fn ellipsis(p: &Painter, r: Rect, c: Color32) {
    for x in [5.0, 12.0, 19.0] {
        p.circle_filled(point(r, x, 12.0), r.width() / 24.0 * 1.5, c);
    }
}
pub fn reload(p: &Painter, r: Rect, c: Color32) {
    let arc: Vec<_> = (0..=30)
        .map(|i| {
            let a =
                -std::f32::consts::FRAC_PI_4 + i as f32 * (5.0 * std::f32::consts::PI / 3.0) / 30.0;
            point(r, 12.0 + 7.0 * a.cos(), 12.0 + 7.0 * a.sin())
        })
        .collect();
    let start = arc[0];
    let end = arc[30];
    p.add(Shape::line(arc, stroke(r, c)));
    p.circle_filled(start, stroke(r, c).width / 2.0, c);
    let angle = -std::f32::consts::FRAC_PI_4 + 5.0 * std::f32::consts::PI / 3.0;
    let tangent = Vec2::new(-angle.sin(), angle.cos()) * (r.width() / 24.0);
    let normal = Vec2::new(angle.cos(), angle.sin()) * (r.width() / 24.0);
    p.line_segment([end - tangent * 4.0 + normal * 2.5, end], stroke(r, c));
    p.line_segment([end - tangent * 4.0 - normal * 2.5, end], stroke(r, c));
    p.circle_filled(end, stroke(r, c).width / 2.0, c);
}
pub fn shield(p: &Painter, r: Rect, c: Color32) {
    path(
        p,
        r,
        c,
        &[
            (12.0, 2.5),
            (20.0, 6.0),
            (19.0, 14.0),
            (16.0, 19.0),
            (12.0, 22.0),
            (8.0, 19.0),
            (5.0, 14.0),
            (4.0, 6.0),
            (12.0, 2.5),
        ],
    );
}
pub fn shield_check(p: &Painter, r: Rect, c: Color32) {
    shield(p, r, c);
    path(p, r, c, &[(8.0, 12.0), (11.0, 15.0), (16.0, 9.0)]);
}
pub fn sidebar_toggle(p: &Painter, r: Rect, c: Color32) {
    p.rect_stroke(
        r.shrink(r.width() * 3.0 / 24.0),
        r.width() * 3.0 / 24.0,
        stroke(r, c),
        StrokeKind::Inside,
    );
    path(p, r, c, &[(10.0, 3.0), (10.0, 21.0)]);
}
pub fn search(p: &Painter, r: Rect, c: Color32) {
    p.circle_stroke(point(r, 10.5, 10.5), r.width() * 6.0 / 24.0, stroke(r, c));
    path(p, r, c, &[(15.0, 15.0), (21.0, 21.0)]);
}
pub fn gear(p: &Painter, r: Rect, c: Color32) {
    p.circle_stroke(point(r, 12.0, 12.0), r.width() * 7.0 / 24.0, stroke(r, c));
    p.circle_stroke(point(r, 12.0, 12.0), r.width() * 2.5 / 24.0, stroke(r, c));
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        path(
            p,
            r,
            c,
            &[
                (12.0 + a.cos() * 8.0, 12.0 + a.sin() * 8.0),
                (12.0 + a.cos() * 10.0, 12.0 + a.sin() * 10.0),
            ],
        );
    }
}
pub fn lock(p: &Painter, r: Rect, c: Color32) {
    p.rect_stroke(
        Rect::from_min_max(point(r, 5.0, 10.0), point(r, 19.0, 21.0)),
        r.width() * 2.0 / 24.0,
        stroke(r, c),
        StrokeKind::Inside,
    );
    path(
        p,
        r,
        c,
        &[
            (8.0, 10.0),
            (8.0, 7.0),
            (9.0, 4.0),
            (12.0, 3.0),
            (15.0, 4.0),
            (16.0, 7.0),
            (16.0, 10.0),
        ],
    );
}

pub fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    button_background(
        ui,
        &response,
        rect,
        ui.visuals().widgets.hovered.bg_fill,
        ui.visuals().selection.bg_fill,
        size * 0.25,
    );
    let color = ui.visuals().text_color();
    icon(ui.painter(), rect.shrink(size * 0.18), color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    focus_ring(
        ui,
        &response,
        rect,
        ui.visuals().selection.stroke.color,
        size * 0.25,
    );
    response
}

pub fn button_background(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: Rect,
    hover_fill: Color32,
    press_fill: Color32,
    radius: f32,
) {
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.12);
    let fill = if response.is_pointer_button_down_on() {
        press_fill
    } else {
        hover_fill.gamma_multiply(hover)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, radius, fill);
    }
}

pub fn focus_ring(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: Rect,
    accent: Color32,
    radius: f32,
) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            radius,
            Stroke::new(2.0, accent),
            StrokeKind::Inside,
        );
    }
}
