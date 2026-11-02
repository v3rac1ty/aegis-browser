// the logo assembles from dots, the aurora blooms, the logo flies to the eyebrow

use eframe::egui::{self, Color32, Mesh, Painter, Pos2, Rect};

use super::intro::ease_out;
use crate::logo;

const DOT_START: f32 = 0.10;
const DOT_STAGGER: f32 = 0.045;
const DOT_TIME: f32 = 0.54;
const GLOW_START: f32 = 1.00;
const GLOW_TIME: f32 = 0.45;
const GLOW_PEAK: f32 = 0.35;
const AURORA_START: f32 = 1.10;
const AURORA_TIME: f32 = 1.20;
const FLIGHT_START: f32 = 1.45;
const FLIGHT_TIME: f32 = 0.70;
// rate of the critically damped spring that carries the logo
const FLIGHT_RATE: f32 = 13.0;
// the page is fully risen and the eyebrow logo can take over
const HANDOFF: f32 = FLIGHT_START + 1.25;
const SKIP_TIME: f32 = 0.15;
// content clock after a skip, past every intro animation
const SKIP_CONTENT: f32 = 3.0;
const LOGO_SIDE: f32 = 96.0;
const GLOW_SEGMENTS: usize = 24;
const GLOW_RINGS: [f32; 4] = [0.3, 0.55, 0.78, 1.0];

/// where each dot starts, in points from its slot
pub const SCATTER: [[f32; 2]; 10] = [
    [0.0, -260.0],
    [-282.0, -103.0],
    [199.0, -93.0],
    [-242.0, 140.0],
    [268.0, 155.0],
    [-35.0, 197.0],
    [-158.0, -274.0],
    [187.0, 33.0],
    [125.0, 217.0],
    [-229.0, 20.0],
];

/// frame time while the opening plays is capped so a hitch can't eat it
pub fn clamp_dt(raw: f32, opening: bool) -> f32 {
    let raw = raw.max(0.0);
    if opening {
        raw.min(1.0 / 30.0)
    } else {
        raw.min(0.1)
    }
}

fn span(clock: f32, start: f32, duration: f32) -> f32 {
    ((clock - start) / duration).clamp(0.0, 1.0)
}

fn smooth(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    (1.0 - amount) * from + amount * to
}

// critically damped spring from rest toward 1, scaled so it lands exactly at the end
fn flight(time: f32) -> f32 {
    if time <= 0.0 {
        return 0.0;
    }
    if time >= FLIGHT_TIME {
        return 1.0;
    }
    let raw = |t: f32| 1.0 - (1.0 + FLIGHT_RATE * t) * (-FLIGHT_RATE * t).exp();
    raw(time) / raw(FLIGHT_TIME)
}

/// everything the opening shows at one moment
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// eased arrival of each dot, 0 scattered to 1 in its slot
    pub dots: [f32; 10],
    /// strength of the bloom behind the logo
    pub glow: f32,
    pub aurora: f32,
    /// 0 at the window center, 1 on the eyebrow
    pub flight: f32,
    /// clock for the page content
    pub content: f32,
}

impl Frame {
    pub const END: Frame = Frame {
        dots: [1.0; 10],
        glow: 0.0,
        aurora: 1.0,
        flight: 1.0,
        content: SKIP_CONTENT,
    };

    pub fn at(clock: f32) -> Self {
        let dots = std::array::from_fn(|index| {
            ease_out(span(
                clock,
                DOT_START + index as f32 * DOT_STAGGER,
                DOT_TIME,
            ))
        });
        let glow = span(clock, GLOW_START, GLOW_TIME);
        Self {
            dots,
            glow: smooth(4.0 * glow * (1.0 - glow)),
            aurora: smooth(span(clock, AURORA_START, AURORA_TIME)),
            flight: flight(clock - FLIGHT_START),
            content: (clock - FLIGHT_START).max(0.0),
        }
    }

    fn blend(self, to: Frame, amount: f32) -> Frame {
        Frame {
            dots: std::array::from_fn(|index| lerp(self.dots[index], to.dots[index], amount)),
            glow: lerp(self.glow, to.glow, amount),
            aurora: lerp(self.aurora, to.aurora, amount),
            flight: lerp(self.flight, to.flight, amount),
            content: lerp(self.content, to.content, amount),
        }
    }
}

struct Skip {
    clock: f32,
    elapsed: f32,
}

/// state of the opening, dropped once the page has taken over
pub struct Opening {
    skip: Option<Skip>,
}

impl Opening {
    pub fn new() -> Self {
        Self { skip: None }
    }

    pub fn skip(&mut self, clock: f32) {
        if self.skip.is_none() {
            self.skip = Some(Skip {
                clock,
                elapsed: 0.0,
            });
        }
    }

    pub fn skipping(&self) -> bool {
        self.skip.is_some()
    }

    pub fn tick(&mut self, dt: f32) {
        if let Some(skip) = &mut self.skip {
            skip.elapsed += dt;
        }
    }

    pub fn frame(&self, clock: f32) -> Frame {
        let Some(skip) = &self.skip else {
            return Frame::at(clock);
        };
        let amount = ease_out((skip.elapsed / SKIP_TIME).clamp(0.0, 1.0));
        let mut frame = Frame::at(skip.clock).blend(Frame::END, amount);
        // the content clock keeps running after the fade
        frame.content += (skip.elapsed - SKIP_TIME).max(0.0);
        frame
    }

    pub fn finished(&self, clock: f32) -> bool {
        match &self.skip {
            Some(skip) => skip.elapsed >= SKIP_TIME,
            None => clock >= HANDOFF,
        }
    }
}

/// the rect of the flying logo, ending exactly on the eyebrow logo
pub fn logo_rect(flight: f32, window: Rect, target: Rect) -> Rect {
    let side = (window.width().min(window.height()) * 0.5).clamp(24.0, LOGO_SIDE);
    let start = Rect::from_center_size(window.center(), egui::vec2(side, side));
    Rect::from_min_max(
        start.min.lerp(target.min, flight),
        start.max.lerp(target.max, flight),
    )
}

/// the dots, white while they gather and the eyebrow color once landed
pub fn paint_dots(painter: &Painter, frame: &Frame, rect: Rect, landed: Color32) {
    let color = Color32::WHITE.lerp_to_gamma(landed, frame.flight);
    let (slots, radius) = logo::layout(rect);
    for (index, slot) in slots.iter().enumerate() {
        let arrival = frame.dots[index];
        if arrival <= 0.0 {
            continue;
        }
        let away = 1.0 - arrival;
        let center = Pos2::new(
            slot.x + SCATTER[index][0] * away,
            slot.y + SCATTER[index][1] * away,
        );
        painter.circle_filled(
            center,
            radius * (0.4 + 0.6 * arrival),
            color.gamma_multiply(arrival),
        );
    }
}

/// a soft radial bloom behind the logo, no edge anywhere
pub fn paint_glow(painter: &Painter, window: Rect, frame: &Frame, accent: Color32) {
    if frame.glow <= 0.0 {
        return;
    }
    let base = (window.width().min(window.height()) * 0.45).clamp(120.0, 420.0);
    let radius = base * (0.75 + 0.25 * frame.glow);
    let center = window.center();
    let strength = GLOW_PEAK * frame.glow;
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(1 + GLOW_RINGS.len() * GLOW_SEGMENTS);
    mesh.reserve_triangles(GLOW_SEGMENTS * (1 + 2 * (GLOW_RINGS.len() - 1)));
    mesh.colored_vertex(center, accent.gamma_multiply(strength));
    for ring in GLOW_RINGS {
        let fade = (1.0 - ring) * (1.0 - ring);
        let color = accent.gamma_multiply(strength * fade);
        for segment in 0..GLOW_SEGMENTS {
            let angle = segment as f32 / GLOW_SEGMENTS as f32 * std::f32::consts::TAU;
            let point = center + egui::vec2(angle.cos(), angle.sin()) * radius * ring;
            mesh.colored_vertex(point, color);
        }
    }
    let segments = GLOW_SEGMENTS as u32;
    for segment in 0..segments {
        let next = (segment + 1) % segments;
        mesh.add_triangle(0, 1 + segment, 1 + next);
    }
    for ring in 0..GLOW_RINGS.len() as u32 - 1 {
        let inner = 1 + ring * segments;
        let outer = inner + segments;
        for segment in 0..segments {
            let next = (segment + 1) % segments;
            mesh.add_triangle(inner + segment, outer + segment, outer + next);
            mesh.add_triangle(inner + segment, outer + next, inner + next);
        }
    }
    painter.add(mesh);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eyebrow(card: Rect) -> Rect {
        super::super::eyebrow_logo_rect(card)
    }

    #[test]
    fn scatter_table_is_far_and_spread_out() {
        let mut quadrants = [false; 4];
        for [x, y] in SCATTER {
            let distance = x.hypot(y);
            assert!((180.0..=320.0).contains(&distance), "{distance}");
            quadrants[usize::from(x >= 0.0) * 2 + usize::from(y >= 0.0)] = true;
        }
        assert!(quadrants.iter().all(|seen| *seen));
    }

    #[test]
    fn dots_start_away_and_settle_on_their_slots() {
        let start = Frame::at(0.0);
        assert!(start.dots.iter().all(|dot| *dot == 0.0));
        let apex = Frame::at(DOT_START + 0.1);
        assert!(apex.dots[0] > apex.dots[1]);
        assert!(apex.dots[1] > apex.dots[9]);
        let settled = Frame::at(1.05);
        assert!(settled.dots.iter().all(|dot| *dot == 1.0));
        let before = Frame::at(1.0);
        assert!(before.dots[9] < 1.0);
        // each dot is monotonic and never overshoots
        for index in 0..10 {
            let mut last = 0.0;
            for step in 0..=120 {
                let dot = Frame::at(step as f32 / 100.0).dots[index];
                assert!((last..=1.0).contains(&dot));
                last = dot;
            }
        }
    }

    #[test]
    fn bloom_and_aurora_stay_soft() {
        assert_eq!(Frame::at(0.9).glow, 0.0);
        assert_eq!(Frame::at(1.5).glow, 0.0);
        let peak = Frame::at(1.225).glow;
        assert!((peak - 1.0).abs() < 0.001);
        assert_eq!(Frame::at(0.0).aurora, 0.0);
        assert_eq!(Frame::at(1.1).aurora, 0.0);
        assert!(Frame::at(1.7).aurora > 0.4 && Frame::at(1.7).aurora < 0.6);
        assert_eq!(Frame::at(2.3).aurora, 1.0);
    }

    #[test]
    fn flight_lands_exactly_without_a_jump() {
        assert_eq!(Frame::at(1.45).flight, 0.0);
        assert_eq!(Frame::at(2.2).flight, 1.0);
        let mut last = 0.0;
        for step in 0..=70 {
            let value = Frame::at(1.45 + step as f32 / 100.0).flight;
            assert!(value >= last && value <= 1.0);
            // the last frames barely move
            if step == 70 {
                assert!(1.0 - last < 0.01);
            }
            last = value;
        }
    }

    #[test]
    fn logo_lands_on_the_eyebrow_at_any_window_size() {
        for (width, height) in [
            (2000.0, 1067.0),
            (1280.0, 800.0),
            (640.0, 480.0),
            (90.0, 60.0),
        ] {
            let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(width, height));
            let card = Rect::from_center_size(window.center(), egui::vec2(1000.0, 560.0));
            let target = eyebrow(card);
            assert_eq!(logo_rect(Frame::at(2.2).flight, window, target), target);
            assert_eq!(logo_rect(1.0, window, target), target);
            let start = logo_rect(0.0, window, target);
            assert_eq!(start.center(), window.center());
            assert!(start.width() <= LOGO_SIDE);
            let middle = logo_rect(0.5, window, target);
            assert!(middle.width() < start.width() && middle.width() > target.width());
        }
    }

    #[test]
    fn content_clock_waits_then_runs() {
        assert_eq!(Frame::at(0.0).content, 0.0);
        assert_eq!(Frame::at(1.45).content, 0.0);
        assert!((Frame::at(2.45).content - 1.0).abs() < 0.0001);
        let mut last = 0.0;
        for step in 0..400 {
            let content = Frame::at(step as f32 / 100.0).content;
            assert!(content >= last);
            last = content;
        }
    }

    #[test]
    fn dt_is_clamped_while_opening() {
        assert_eq!(clamp_dt(0.5, true), 1.0 / 30.0);
        assert_eq!(clamp_dt(0.01, true), 0.01);
        assert_eq!(clamp_dt(0.5, false), 0.1);
        assert_eq!(clamp_dt(-1.0, true), 0.0);
    }

    #[test]
    fn skip_ends_where_the_full_run_ends() {
        let end = Frame::at(100.0);
        assert_eq!(end.dots, Frame::END.dots);
        assert_eq!(end.flight, Frame::END.flight);
        assert_eq!(end.aurora, Frame::END.aurora);
        assert!(end.glow.abs() < 0.000_001);
        for skip_at in [0.0, 0.3, 1.2, 1.9, 2.5] {
            let mut opening = Opening::new();
            opening.skip(skip_at);
            let mut last_content = Frame::at(skip_at).content;
            for _ in 0..60 {
                opening.tick(1.0 / 240.0);
                let frame = opening.frame(skip_at);
                assert!(frame.content >= last_content);
                last_content = frame.content;
            }
            assert!(opening.finished(skip_at));
            let frame = opening.frame(skip_at);
            assert_eq!(frame.dots, Frame::END.dots);
            assert_eq!(frame.flight, 1.0);
            assert_eq!(frame.aurora, 1.0);
            assert_eq!(frame.glow, 0.0);
            assert!(frame.content >= SKIP_CONTENT);
        }
    }

    #[test]
    fn skip_fades_instead_of_cutting() {
        let mut opening = Opening::new();
        opening.skip(0.6);
        let before = Frame::at(0.6);
        opening.tick(0.0);
        let same = opening.frame(0.6);
        assert!((same.aurora - before.aurora).abs() < 0.000_01);
        assert!((same.dots[9] - before.dots[9]).abs() < 0.000_01);
        assert_eq!(same.content, before.content);
        opening.tick(0.05);
        let middle = opening.frame(0.6);
        assert!(middle.aurora > before.aurora && middle.aurora < 1.0);
        assert!(middle.dots[9] > before.dots[9] && middle.dots[9] < 1.0);
        assert!(!opening.finished(0.6));
    }

    #[test]
    fn unskipped_opening_finishes_when_the_page_has_risen() {
        let opening = Opening::new();
        assert!(!opening.finished(2.69));
        assert!(opening.finished(2.7));
        assert!(!opening.skipping());
    }
}
