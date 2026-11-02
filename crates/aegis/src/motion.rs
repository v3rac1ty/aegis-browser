use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use std::time::Instant;

use aegis_core::{FrameLimit, MotionPref};
use eframe::egui::{self, Id};

static FRAME_NANOS: AtomicU64 = AtomicU64::new(16_666_667);
static DISPLAY_MHZ: AtomicU32 = AtomicU32::new(60_000);
static FRAME_LIMIT: AtomicU8 = AtomicU8::new(0);
static CUSTOM_FPS: AtomicU16 = AtomicU16::new(120);

fn frame_interval(limit: FrameLimit, custom_fps: u16, display_hz: f32) -> Option<Duration> {
    let hz = match limit {
        FrameLimit::Display => {
            if display_hz.is_finite() {
                display_hz.clamp(30.0, 240.0)
            } else {
                60.0
            }
        }
        FrameLimit::Custom => f32::from(custom_fps.clamp(30, 500)),
        FrameLimit::Vsync | FrameLimit::Uncapped => return None,
    };
    Some(Duration::from_secs_f32(1.0 / hz))
}

pub fn current_frame_interval() -> Option<Duration> {
    let nanos = FRAME_NANOS.load(Ordering::Relaxed);
    (nanos != 0).then(|| Duration::from_nanos(nanos))
}

pub fn request_animation_frame(ctx: &egui::Context) {
    if let Some(interval) = current_frame_interval() {
        ctx.request_repaint_after(interval);
    } else {
        ctx.request_repaint();
    }
}

fn update_interval() {
    let limit = match FRAME_LIMIT.load(Ordering::Relaxed) {
        1 => FrameLimit::Vsync,
        2 => FrameLimit::Custom,
        3 => FrameLimit::Uncapped,
        _ => FrameLimit::Display,
    };
    let hz = DISPLAY_MHZ.load(Ordering::Relaxed) as f32 / 1000.0;
    let nanos = frame_interval(limit, CUSTOM_FPS.load(Ordering::Relaxed), hz)
        .map_or(0, |interval| interval.as_nanos() as u64);
    FRAME_NANOS.store(nanos, Ordering::Relaxed);
}

pub fn set_frame_limit(limit: FrameLimit, custom_fps: u16) {
    let value = match limit {
        FrameLimit::Display => 0,
        FrameLimit::Vsync => 1,
        FrameLimit::Custom => 2,
        FrameLimit::Uncapped => 3,
    };
    FRAME_LIMIT.store(value, Ordering::Relaxed);
    CUSTOM_FPS.store(custom_fps, Ordering::Relaxed);
    update_interval();
}

pub fn set_refresh_rate(millihertz: Option<u32>) {
    DISPLAY_MHZ.store(millihertz.unwrap_or(60_000), Ordering::Relaxed);
    update_interval();
}

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
        }
    }

    pub fn settled(&self) -> bool {
        (self.value - self.target).abs() < 0.001 && self.velocity.abs() < 0.001
    }

    pub fn step(&mut self, dt: f32, response: f32) {
        if dt <= 0.0 || self.settled() {
            return;
        }
        let omega = std::f32::consts::TAU / response;
        let displacement = self.value - self.target;
        let coefficient = self.velocity + omega * displacement;
        let decay = (-omega * dt).exp();
        let next = self.target + (displacement + coefficient * dt) * decay;
        let velocity = (self.velocity - omega * coefficient * dt) * decay;
        if (self.target - self.value).signum() != (self.target - next).signum() {
            self.value = self.target;
            self.velocity = 0.0;
        } else {
            self.value = next;
            self.velocity = velocity;
        }
        if self.settled() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }
}

struct Fade {
    start: f32,
    target: f32,
    elapsed: f32,
}

pub struct Motion {
    springs: HashMap<Id, Spring>,
    fades: HashMap<Id, Fade>,
    last_frame: Instant,
    reduced: bool,
    focused: bool,
}

impl Default for Motion {
    fn default() -> Self {
        Self::new()
    }
}

impl Motion {
    pub fn new() -> Self {
        Self {
            springs: HashMap::new(),
            fades: HashMap::new(),
            last_frame: Instant::now(),
            reduced: aegis_ffi::reduce_motion(),
            focused: true,
        }
    }

    pub fn frame(&mut self) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        dt
    }

    pub fn seed(&mut self, id: Id, value: f32) {
        self.springs.insert(id, Spring::new(value));
        self.fades.remove(&id);
    }

    pub fn value(
        &mut self,
        ctx: &egui::Context,
        id: Id,
        target: f32,
        response: f32,
        dt: f32,
        preference: MotionPref,
    ) -> f32 {
        let focused = ctx.input(|input| input.focused);
        if focused && !self.focused && ctx.viewport_id() == egui::ViewportId::ROOT {
            self.reduced = aegis_ffi::reduce_motion();
        }
        self.focused = focused;
        let reduced = match preference {
            MotionPref::Reduced => true,
            MotionPref::Full => false,
            MotionPref::System => self.reduced,
        };
        let spring = self
            .springs
            .entry(id)
            .or_insert_with(|| Spring::new(target));
        if reduced {
            let fade = self.fades.entry(id).or_insert(Fade {
                start: spring.value,
                target,
                elapsed: 0.0,
            });
            if fade.target != target {
                *fade = Fade {
                    start: spring.value,
                    target,
                    elapsed: 0.0,
                };
            }
            fade.elapsed = (fade.elapsed + dt).min(0.12);
            spring.value = fade.start + (fade.target - fade.start) * (fade.elapsed / 0.12);
            spring.velocity = 0.0;
            spring.target = target;
            if fade.elapsed < 0.12 && (fade.target - fade.start).abs() >= 0.001 {
                request_animation_frame(ctx);
            }
        } else {
            self.fades.remove(&id);
            spring.target = target;
            spring.step(dt, response);
            if !spring.settled() {
                request_animation_frame(ctx);
            }
        }
        spring.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_intervals_follow_mode() {
        assert_eq!(
            frame_interval(FrameLimit::Display, 120, 60.0),
            Some(Duration::from_secs_f32(1.0 / 60.0))
        );
        assert_eq!(
            frame_interval(FrameLimit::Display, 120, 144.0),
            Some(Duration::from_secs_f32(1.0 / 144.0))
        );
        assert_eq!(
            frame_interval(FrameLimit::Display, 120, f32::NAN),
            Some(Duration::from_secs_f32(1.0 / 60.0))
        );
        assert_eq!(
            frame_interval(FrameLimit::Custom, 120, 60.0),
            Some(Duration::from_secs_f32(1.0 / 120.0))
        );
        assert_eq!(
            frame_interval(FrameLimit::Custom, 0, 60.0),
            Some(Duration::from_secs_f32(1.0 / 30.0))
        );
        assert_eq!(frame_interval(FrameLimit::Vsync, 120, 60.0), None);
        assert_eq!(frame_interval(FrameLimit::Uncapped, 120, 60.0), None);
    }

    #[test]
    fn spring_settles_without_overshoot() {
        let mut spring = Spring::new(0.0);
        spring.target = 1.0;
        for _ in 0..120 {
            spring.step(1.0 / 60.0, 0.34);
            assert!((0.0..=1.0).contains(&spring.value));
        }
        assert!(spring.settled());
        assert_eq!(spring.value, 1.0);
    }

    #[test]
    fn zero_dt_does_not_move() {
        let mut spring = Spring::new(0.0);
        spring.target = 1.0;
        spring.step(0.0, 0.2);
        assert_eq!(spring.value, 0.0);
        assert_eq!(spring.velocity, 0.0);
    }
}
