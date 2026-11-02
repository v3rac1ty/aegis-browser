//! set AEGIS_DEBUG_WELCOME=1 in a debug build to replay first run

mod aurora;
mod hero;
mod intro;
mod opening;
mod projection;

use std::borrow::Cow;
use std::time::Instant;

use aegis_core::{GroupColor, Settings, Setup, ThemeMode};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind};

use crate::accent_picker::AccentPicker;
use crate::glass;
use crate::motion::{self, Motion, Spring};
use crate::preview::{self, MiniColors};
use crate::protections::{self, ProtectionKind};
use crate::settings_page::{self, Change};
use crate::theme::{self, Theme};
use crate::widgets;

#[derive(Default)]
pub struct StepState {
    index: usize,
}

impl StepState {
    fn next(&mut self, settings: &mut Settings) -> bool {
        if self.index == 3 {
            settings.welcome_done = true;
            return true;
        }
        self.index += 1;
        false
    }

    fn back(&mut self) {
        self.index = self.index.saturating_sub(1);
    }
}

struct Transition {
    outgoing: Option<usize>,
    direction: f32,
    progress: Spring,
}

impl Transition {
    fn new() -> Self {
        Self {
            outgoing: None,
            direction: 1.0,
            progress: Spring::new(1.0),
        }
    }

    fn start(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        self.outgoing = Some(from);
        self.direction = if to > from { 1.0 } else { -1.0 };
        self.progress = Spring::new(0.0);
        self.progress.target = 1.0;
    }

    fn frame(&mut self, dt: f32, reduced: bool) {
        if self.outgoing.is_none() {
            return;
        }
        if reduced {
            self.progress.value = (self.progress.value + dt / 0.12).min(1.0);
        } else {
            self.progress.step(dt, 0.36);
        }
        if self.progress.value >= 0.999 {
            self.outgoing = None;
            self.progress.value = 1.0;
        }
    }
}

fn outgoing_opacity(progress: f32) -> f32 {
    1.0 - intro::ease_out((progress / 0.35).clamp(0.0, 1.0))
}

fn percent_text(value: u8, buffer: &mut [u8; 4]) -> &str {
    let mut len = 0;
    if value >= 100 {
        buffer[len] = b'0' + value / 100;
        len += 1;
    }
    if value >= 10 {
        buffer[len] = b'0' + value / 10 % 10;
        len += 1;
    }
    buffer[len] = b'0' + value % 10;
    len += 1;
    buffer[len] = b'%';
    std::str::from_utf8(&buffer[..=len]).unwrap_or("0%")
}

/// everything the welcome keeps in egui lives under this id
fn root_id() -> egui::Id {
    egui::Id::unique("aegis_welcome")
}

/// whether this launch builds the welcome at all
pub fn should_show(welcome_done: bool, debug_replay: bool) -> bool {
    !welcome_done || debug_replay
}

/// drop the welcome, then clear what it left in egui
pub fn finish(slot: &mut Option<Welcome>, ctx: &egui::Context) {
    if let Some(welcome) = slot.take() {
        welcome.release(ctx);
    }
}

// the page logo is 18 pt, top left of the content
fn content_top(card_height: f32) -> f32 {
    44.0 + ((card_height - 44.0 - 46.0 - 28.0 - 414.0) * 0.5).max(0.0)
}

fn eyebrow_logo_rect(card: Rect) -> Rect {
    Rect::from_min_size(
        card.min + egui::vec2(52.0, content_top(card.height())),
        egui::vec2(18.0, 18.0),
    )
}

fn eyebrow_color(accent: Color32) -> Color32 {
    Color32::from_rgb(
        (f32::from(accent.r()) * 0.45 + 255.0 * 0.55) as u8,
        (f32::from(accent.g()) * 0.45 + 255.0 * 0.55) as u8,
        (f32::from(accent.b()) * 0.45 + 255.0 * 0.55) as u8,
    )
}

// any press or key during the opening skips it
fn skip_requested(ctx: &egui::Context) -> bool {
    ctx.input(|input| {
        input.pointer.any_pressed()
            || input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }))
    })
}

// the atlas never shrinks, so start a new one
fn rebuild_font_atlas(ctx: &egui::Context) {
    let policy = ctx.missing_glyph_policy();
    let other = if policy == egui::MissingGlyphPolicy::Tofu {
        egui::MissingGlyphPolicy::Panic
    } else {
        egui::MissingGlyphPolicy::Tofu
    };
    ctx.set_missing_glyph_policy(other);
    ctx.set_missing_glyph_policy(policy);
}

fn clip_polygon_to_rounded_rect(points: &[Pos2], rim: &[Pos2]) -> Vec<Pos2> {
    let mut inside = points.to_vec();
    let mut output = Vec::with_capacity(rim.len() + points.len());
    for index in 0..rim.len() {
        let a = rim[index];
        let b = rim[(index + 1) % rim.len()];
        output.clear();
        if inside.is_empty() {
            break;
        }
        let mut previous = inside[inside.len() - 1];
        let mut previous_side = (b.x - a.x) * (previous.y - a.y) - (b.y - a.y) * (previous.x - a.x);
        for &point in &inside {
            let side = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
            if (side >= 0.0) != (previous_side >= 0.0) {
                let t = previous_side / (previous_side - side);
                output.push(previous.lerp(point, t));
            }
            if side >= 0.0 {
                output.push(point);
            }
            previous = point;
            previous_side = side;
        }
        std::mem::swap(&mut inside, &mut output);
    }
    inside
}

pub struct Welcome {
    state: StepState,
    aurora: aurora::Aurora,
    hero: hero::Hero,
    transition: Transition,
    last_frame: Option<Instant>,
    // seconds since the first frame, drives the aurora drift
    clock: f32,
    // clock for the page content, waits for the opening
    content: f32,
    opening: Option<opening::Opening>,
    height: Spring,
    setup_previews: Option<Vec<Settings>>,
    last_accent: Option<[u8; 3]>,
    last_aurora: Option<Instant>,
    accent_picker: AccentPicker,
    setup_motion: Motion,
    setup_dt: f32,
    #[cfg(debug_assertions)]
    finish_at: Option<f32>,
}

impl Welcome {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            state: StepState::default(),
            aurora: aurora::Aurora::new(ctx),
            hero: hero::Hero::new(),
            transition: Transition::new(),
            last_frame: None,
            clock: 0.0,
            content: 0.0,
            opening: Some(opening::Opening::new()),
            height: Spring::new(560.0),
            setup_previews: None,
            last_accent: None,
            last_aurora: None,
            accent_picker: AccentPicker::default(),
            setup_motion: Motion::new(),
            setup_dt: 0.0,
            #[cfg(debug_assertions)]
            finish_at: std::env::var("AEGIS_DEBUG_WELCOME_FINISH_AT")
                .ok()
                .and_then(|value| crate::debug_shot::parse_seconds(&value)),
        }
    }

    /// frees the textures, then clears egui state and the font atlas
    pub fn release(self, ctx: &egui::Context) {
        drop(self);
        // area state and animations are all the welcome keeps in memory
        ctx.memory_mut(|memory| {
            memory.reset_areas();
            memory.data.clear();
            memory.caches = Default::default();
        });
        ctx.clear_animations();
        rebuild_font_atlas(ctx);
    }

    fn finished(ctx: &egui::Context, settings: Settings) -> (Option<Settings>, bool) {
        ctx.tessellation_options_mut(|options| {
            options.round_text_to_pixels = true;
            options.round_rects_to_pixels = true;
            options.round_line_segments_to_pixels = true;
        });
        (Some(settings), true)
    }

    fn enter_step(&mut self, settings: &Settings) {
        if self.state.index == 1 && self.setup_previews.is_none() {
            let mut previews = Vec::with_capacity(5);
            for setup in [
                Setup::Explorer,
                Setup::Focus,
                Setup::Power,
                Setup::Minimal,
                Setup::Classic,
            ] {
                let mut sample = settings.clone();
                sample.apply_setup(setup);
                previews.push(sample);
            }
            self.setup_previews = Some(previews);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        settings: &Settings,
        _theme: Theme,
        _space: GroupColor,
        _glass_supported: bool,
        grain: &egui::TextureHandle,
    ) -> (Option<Settings>, bool) {
        let ctx = ui.ctx().clone();
        let mut next = Cow::Borrowed(settings);
        self.enter_step(settings);
        let now = Instant::now();
        // the clock starts on the first frame, so startup work isn't counted
        let raw_dt = self
            .last_frame
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
        self.last_frame = Some(now);
        let reduced = settings.motion == aegis_core::MotionPref::Reduced
            || (settings.motion == aegis_core::MotionPref::System && aegis_ffi::reduce_motion());
        if reduced {
            self.opening = None;
        }
        let dt = opening::clamp_dt(raw_dt, self.opening.is_some());
        self.setup_dt = dt;
        self.clock += dt;
        let mut aurora_strength = 1.0;
        let mut overlay = None;
        if let Some(opening) = &mut self.opening {
            if skip_requested(&ctx) {
                opening.skip(self.clock);
            }
            opening.tick(dt);
            let frame = opening.frame(self.clock);
            self.content = frame.content;
            aurora_strength = frame.aurora;
            if opening.finished(self.clock) {
                self.opening = None;
            } else {
                overlay = Some(frame);
            }
        } else if !reduced {
            self.content += dt;
        }
        let time = if reduced { 0.0 } else { self.content };
        self.transition.frame(dt, reduced);
        let moving = (!reduced && time < 3.0) || self.transition.outgoing.is_some();
        ctx.tessellation_options_mut(|options| {
            options.round_text_to_pixels = !moving;
            options.round_rects_to_pixels = !moving && self.state.index != 0;
            options.round_line_segments_to_pixels = !moving && self.state.index != 0;
        });
        let appearance = theme::resolve(settings, true);
        let theme = Theme {
            palette: theme::Palette::dark(),
            dark: true,
            ..appearance
        };
        let window = ui.max_rect();
        let aurora_due = self
            .last_aurora
            .is_none_or(|last| now.duration_since(last).as_secs_f32() >= 1.0 / 30.0);
        let aurora_time = if reduced { 0.0 } else { self.clock };
        if aurora_due
            && (!reduced || self.last_accent != Some(settings.accent_rgb()))
            && self.aurora.update(
                aurora_time,
                theme.accent,
                window.width() / window.height(),
                aurora_strength,
            )
        {
            self.last_accent = Some(settings.accent_rgb());
            self.last_aurora = Some(now);
        }
        self.aurora.paint_background(ui.painter(), window);
        if let Some(frame) = &overlay {
            opening::paint_glow(ui.painter(), window, frame, theme.accent);
        }
        glass::paint_noise(
            ui.painter(),
            window,
            22,
            true,
            grain,
            ctx.pixels_per_point(),
        );
        let card_width = 1060.0_f32.min((window.width() - 48.0).max(320.0));
        let content_height = match self.state.index {
            0 => 414.0_f32,
            1 => 430.0,
            2 => 460.0,
            _ => 422.0,
        };
        self.height.target = (content_height + 44.0 + 46.0 + 28.0)
            .max(560.0)
            .min((window.height() - 96.0).max(420.0));
        if reduced {
            self.height.value = self.height.target;
        } else {
            self.height.step(dt, 0.36);
        }
        let card_height = self.height.value;
        let card = Rect::from_center_size(window.center(), egui::vec2(card_width, card_height));
        let card_pose = intro::rise(time, 0.25, 1.0, 28.0, 0.955, reduced);
        let card = if self.state.index == 0 {
            Rect::from_center_size(
                card.center() + egui::vec2(0.0, card_pose.offset),
                card.size() * card_pose.scale,
            )
        } else {
            card
        };
        let mut advance = ctx.input(|input| input.key_pressed(egui::Key::Enter));
        let mut back = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        if self.state.index == 1 {
            let setups = [
                Setup::Explorer,
                Setup::Focus,
                Setup::Power,
                Setup::Minimal,
                Setup::Classic,
            ];
            if let Some(index) = setups.iter().position(|setup| *setup == settings.setup) {
                let left = ctx.input(|input| input.key_pressed(egui::Key::ArrowLeft));
                let right = ctx.input(|input| input.key_pressed(egui::Key::ArrowRight));
                if left || right {
                    let selected = if left {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(4)
                    };
                    next.to_mut().apply_setup(setups[selected]);
                }
            }
        }
        let skipping = self
            .opening
            .as_ref()
            .is_some_and(opening::Opening::skipping);
        egui::Area::new(root_id().with("card"))
            .order(egui::Order::Foreground)
            .fixed_pos(card.min)
            .show(&ctx, |ui| {
                ui.set_min_size(card.size());
                if self.state.index == 0 {
                    ui.set_opacity(card_pose.opacity);
                }
                ui.painter().add(
                    egui::epaint::Shadow {
                        offset: [0, 50],
                        blur: 120,
                        spread: 0,
                        color: Color32::from_black_alpha(140),
                    }
                    .as_shape(card, 34),
                );
                self.aurora.paint_frost(ui.painter(), card, window);
                ui.painter()
                    .rect_filled(card, 34.0, Color32::from_white_alpha(19));
                ui.painter().rect_stroke(
                    card,
                    34.0,
                    Stroke::new(1.0, Color32::from_white_alpha(41)),
                    StrokeKind::Inside,
                );
                ui.painter().line_segment(
                    [
                        card.left_top() + egui::vec2(34.0, 1.0),
                        card.right_top() + egui::vec2(-34.0, 1.0),
                    ],
                    Stroke::new(1.0, Color32::from_white_alpha(71)),
                );
                let progress = self.transition.progress.value;
                if let Some(outgoing) = self.transition.outgoing {
                    self.draw_page(
                        ui,
                        card,
                        outgoing,
                        if reduced {
                            0.0
                        } else {
                            -40.0 * progress * self.transition.direction
                        },
                        outgoing_opacity(progress),
                        false,
                        &mut next,
                        theme,
                        time,
                        reduced,
                        grain,
                    );
                }
                let incoming_offset = if self.transition.outgoing.is_some() {
                    if reduced {
                        0.0
                    } else {
                        40.0 * (1.0 - progress) * self.transition.direction
                    }
                } else {
                    0.0
                };
                let incoming_opacity = if self.transition.outgoing.is_some() {
                    ((progress - 0.35) / 0.65).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                self.draw_page(
                    ui,
                    card,
                    self.state.index,
                    incoming_offset,
                    incoming_opacity,
                    true,
                    &mut next,
                    theme,
                    time,
                    reduced,
                    grain,
                );
                if self.state.index == 0
                    && let Some(progress) = intro::shine(time, reduced || skipping)
                {
                    self.shine(ui, card, progress);
                }
                self.footer(ui, card, time, reduced, &mut advance, &mut back);
                // the flying logo sits above the card at full opacity
                if let Some(frame) = &overlay {
                    let rect = opening::logo_rect(frame.flight, window, eyebrow_logo_rect(card));
                    let mut painter = ui.painter().clone();
                    painter.set_opacity(1.0);
                    opening::paint_dots(&painter, frame, rect, eyebrow_color(theme.accent));
                }
            });
        // presses during the opening skip it, they never reach the page
        if self.opening.is_some() {
            advance = false;
            back = false;
        }
        #[cfg(debug_assertions)]
        if let Some(at) = self.finish_at {
            if self.clock >= at {
                let mut done = settings.clone();
                done.welcome_done = true;
                return Self::finished(&ctx, done);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(at - self.clock));
        }
        if back {
            let from = self.state.index;
            self.state.back();
            self.transition.start(from, self.state.index);
        }
        if advance {
            let from = self.state.index;
            let finished = self.state.next(next.to_mut());
            self.transition.start(from, self.state.index);
            self.enter_step(next.as_ref());
            if finished {
                return Self::finished(&ctx, next.into_owned());
            }
            return (Some(next.into_owned()), false);
        }
        if !reduced || self.transition.outgoing.is_some() {
            motion::request_animation_frame(&ctx);
        }
        (
            if next.as_ref() != settings {
                Some(next.into_owned())
            } else {
                None
            },
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_page(
        &mut self,
        ui: &mut egui::Ui,
        card: Rect,
        index: usize,
        offset: f32,
        opacity: f32,
        interactive: bool,
        next: &mut Cow<'_, Settings>,
        theme: Theme,
        time: f32,
        reduced: bool,
        grain: &egui::TextureHandle,
    ) {
        let clip = Rect::from_min_max(
            card.min + egui::vec2(36.0, 26.0),
            card.max - egui::vec2(36.0, 82.0),
        );
        ui.push_id(("welcome_step", index), |ui| {
            ui.set_clip_rect(clip);
            ui.set_opacity(opacity);
            if !interactive {
                ui.disable();
            }
            if index == 0 {
                self.step_one(
                    ui,
                    card.translate(egui::vec2(offset, 0.0)),
                    theme,
                    time,
                    reduced,
                    opacity,
                );
                return;
            }
            let height = match index {
                1 => 430.0,
                2 => 460.0,
                _ => 422.0,
            };
            let top = 44.0 + ((card.height() - 44.0 - 46.0 - 28.0 - height) * 0.5).max(0.0);
            let content = Rect::from_min_size(
                card.min + egui::vec2(52.0 + offset, top),
                egui::vec2(card.width() - 104.0, height),
            );
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(widgets::non_negative(content)),
                |ui| match index {
                    1 => self.step_two(ui, next, theme),
                    2 => self.step_three(ui, next, theme, grain),
                    _ => self.step_four(ui, theme),
                },
            );
        });
    }

    fn step_one(
        &mut self,
        ui: &mut egui::Ui,
        card: Rect,
        theme: Theme,
        time: f32,
        reduced: bool,
        opacity: f32,
    ) {
        let top = content_top(card.height());
        let left = card.min + egui::vec2(52.0, top);
        let eyebrow = intro::rise(time, 0.55, 0.9, 26.0, 1.0, reduced);
        let line_one = intro::rise(time, 0.55, 0.9, 26.0, 1.0, reduced);
        let line_two = intro::rise(time, 0.70, 0.9, 26.0, 1.0, reduced);
        let subline = intro::rise(time, 0.90, 0.9, 26.0, 1.0, reduced);
        let text =
            |ui: &egui::Ui, pos: Pos2, words: &str, size: f32, color: Color32, opacity: f32| {
                ui.painter().text(
                    pos,
                    egui::Align2::LEFT_TOP,
                    words,
                    egui::FontId::proportional(size),
                    color.gamma_multiply(opacity),
                );
            };
        let eyebrow_color = eyebrow_color(theme.accent);
        // while the opening plays, the flying logo is drawn instead
        if self.opening.is_none() {
            crate::logo::paint(ui.painter(), eyebrow_logo_rect(card), eyebrow_color);
        }
        text(
            ui,
            left + egui::vec2(28.0, eyebrow.offset),
            "Welcome to Aegis",
            14.0,
            eyebrow_color,
            eyebrow.opacity,
        );
        text(
            ui,
            left + egui::vec2(0.0, 58.0 + line_one.offset),
            "Take control",
            52.0,
            Color32::WHITE,
            line_one.opacity,
        );
        text(
            ui,
            left + egui::vec2(0.0, 113.0 + line_two.offset),
            "of your browser.",
            52.0,
            Color32::WHITE,
            line_two.opacity,
        );
        text(
            ui,
            left + egui::vec2(0.0, 211.0 + subline.offset),
            "Make it yours.",
            20.0,
            Color32::from_white_alpha(184),
            subline.opacity,
        );
        text(
            ui,
            left + egui::vec2(135.0, 211.0 + subline.offset),
            "Keep it yours.",
            20.0,
            Color32::WHITE,
            subline.opacity,
        );
        for (index, label) in ["Clean", "Light", protections::welcome_chip()]
            .into_iter()
            .enumerate()
        {
            let pose = intro::chip(time, index, reduced);
            let width = if index == 2 { 104.0 } else { 92.0 };
            let base = Rect::from_min_size(
                left + egui::vec2(index as f32 * 106.0, 270.0),
                egui::vec2(width, 34.0),
            );
            let chip = Rect::from_center_size(base.center(), base.size() * pose.scale);
            ui.painter().rect_filled(
                chip,
                17.0,
                Color32::from_white_alpha((26.0 * pose.opacity) as u8),
            );
            ui.painter().rect_stroke(
                chip,
                17.0,
                Stroke::new(1.0, Color32::from_white_alpha((36.0 * pose.opacity) as u8)),
                StrokeKind::Inside,
            );
            let icon = Rect::from_min_size(
                chip.left_center() + egui::vec2(11.0, -8.0),
                egui::vec2(16.0, 16.0),
            );
            let ink = Color32::WHITE.gamma_multiply(pose.opacity);
            if index == 0 {
                crate::icons::sparkle(ui.painter(), icon, ink);
            } else if index == 1 {
                crate::icons::feather(ui.painter(), icon, ink);
            } else {
                crate::icons::shield(ui.painter(), icon, ink);
            }
            text(
                ui,
                chip.left_center() + egui::vec2(32.0, -9.0),
                label,
                14.0 * pose.scale,
                Color32::WHITE,
                pose.opacity,
            );
        }
        let hero_rect = Rect::from_min_size(
            card.min + egui::vec2(card.width() * 0.54, top + 12.0),
            egui::vec2(card.width() * 0.42, 390.0),
        );
        let hero_pose = intro::rise(time, 0.70, 1.3, 60.0, 1.0, reduced);
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(widgets::non_negative(hero_rect)),
            |ui| {
                ui.set_opacity(hero_pose.opacity * opacity);
                self.hero.show(
                    ui,
                    hero_rect,
                    theme.accent,
                    time,
                    reduced,
                    1.0 - hero_pose.opacity,
                );
            },
        );
    }

    fn step_two(&mut self, ui: &mut egui::Ui, next: &mut Cow<'_, Settings>, theme: Theme) {
        ui.heading(
            egui::RichText::new("Pick a starting setup")
                .size(32.0)
                .color(Color32::WHITE),
        );
        ui.label(
            egui::RichText::new("Every part of it can be changed later in Settings, Sidebar.")
                .color(Color32::from_white_alpha(184)),
        );
        ui.add_space(35.0);
        let blurbs = [
            "Everything, nicely spaced",
            "Hides until hovered",
            "Dense, on the right",
            "Just tabs",
            "Tabs across the top",
        ];
        let setups = [
            Setup::Explorer,
            Setup::Focus,
            Setup::Power,
            Setup::Minimal,
            Setup::Classic,
        ];
        if let Some(previews) = &mut self.setup_previews {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let width = ((ui.available_width() - 40.0) / 5.0).max(100.0);
                for (index, settings) in previews.iter().enumerate() {
                    let selected = next.setup == setups[index];
                    let card = ui.allocate_exact_size(egui::vec2(width, 315.0), Sense::click());
                    let selection = self.setup_motion.value(
                        ui.ctx(),
                        ui.scope_id().with(("setup_selected", index)),
                        if selected { 1.0 } else { 0.0 },
                        0.24,
                        self.setup_dt,
                        next.motion,
                    );
                    let hover = self.setup_motion.value(
                        ui.ctx(),
                        ui.scope_id().with(("setup_hover", index)),
                        if card.1.hovered() { 1.0 } else { 0.0 },
                        0.24,
                        self.setup_dt,
                        next.motion,
                    );
                    let rect = card
                        .0
                        .translate(egui::vec2(0.0, -6.0 * selection - 2.0 * hover));
                    ui.painter().rect_filled(
                        rect,
                        16.0,
                        Color32::from_white_alpha((22.0 + 16.0 * selection) as u8),
                    );
                    ui.painter().rect_stroke(
                        rect,
                        16.0,
                        Stroke::new(1.0 + selection, theme.accent.gamma_multiply(selection)),
                        StrokeKind::Inside,
                    );
                    let preview_rect = Rect::from_min_size(
                        rect.min + egui::vec2(7.0, 7.0),
                        egui::vec2(width - 14.0, 224.0),
                    );
                    preview::draw_mini(
                        ui.painter(),
                        preview_rect,
                        settings,
                        &MiniColors::from(theme),
                    );
                    ui.painter().text(
                        rect.min + egui::vec2(12.0, 241.0),
                        egui::Align2::LEFT_TOP,
                        setups[index].label(),
                        egui::FontId::proportional(16.0),
                        Color32::WHITE,
                    );
                    ui.painter().text(
                        rect.min + egui::vec2(12.0, 271.0),
                        egui::Align2::LEFT_TOP,
                        blurbs[index],
                        egui::FontId::proportional(11.0),
                        Color32::from_white_alpha(178),
                    );
                    if selection > 0.001 {
                        let center = rect.right_top() + egui::vec2(-14.0, 14.0);
                        ui.painter().circle_filled(
                            center,
                            9.0 * (0.6 + 0.4 * selection),
                            theme.accent.gamma_multiply(selection),
                        );
                        ui.painter().line_segment(
                            [
                                center + egui::vec2(-4.0, 0.0),
                                center + egui::vec2(-1.0, 3.0),
                            ],
                            Stroke::new(1.5, Color32::WHITE.gamma_multiply(selection)),
                        );
                        ui.painter().line_segment(
                            [
                                center + egui::vec2(-1.0, 3.0),
                                center + egui::vec2(5.0, -4.0),
                            ],
                            Stroke::new(1.5, Color32::WHITE.gamma_multiply(selection)),
                        );
                    }
                    card.1.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, setups[index].label())
                    });
                    if card.1.clicked() {
                        next.to_mut().apply_setup(setups[index]);
                    }
                }
            });
        }
    }

    fn step_three(
        &mut self,
        ui: &mut egui::Ui,
        next: &mut Cow<'_, Settings>,
        theme: Theme,
        grain: &egui::TextureHandle,
    ) {
        ui.heading(
            egui::RichText::new("Make it yours")
                .size(32.0)
                .color(Color32::WHITE),
        );
        ui.label(
            egui::RichText::new("The preview updates as you choose.")
                .color(Color32::from_white_alpha(184)),
        );
        ui.add_space(25.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(340.0);
                ui.label(
                    egui::RichText::new("Accent")
                        .size(13.0)
                        .strong()
                        .color(Color32::from_white_alpha(204)),
                );
                let preference = next.motion;
                self.accent_picker.show(ui, next.to_mut(), 34.0, preference);
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new("Appearance")
                        .size(13.0)
                        .strong()
                        .color(Color32::from_white_alpha(204)),
                );
                if let Some(index) = widgets::segmented(
                    ui,
                    "Appearance",
                    &["Auto", "Light", "Dark"],
                    ThemeMode::ALL
                        .iter()
                        .position(|value| *value == next.theme)
                        .unwrap_or(0),
                    theme,
                ) {
                    settings_page::apply(next.to_mut(), Change::Theme(ThemeMode::ALL[index]));
                }
                ui.add_space(24.0);
                let mut noise = next.noise;
                let mut percent_buffer = [0_u8; 4];
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Glass noise")
                            .size(13.0)
                            .strong()
                            .color(Color32::from_white_alpha(204)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(percent_text(noise, &mut percent_buffer))
                                .color(Color32::WHITE),
                        );
                    });
                });
                if ui
                    .add_sized(
                        [340.0, 20.0],
                        egui::Slider::new(&mut noise, 0..=100)
                            .step_by(5.0)
                            .show_value(false),
                    )
                    .changed()
                {
                    settings_page::apply(next.to_mut(), Change::Noise(noise));
                }
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("Sidebar opacity")
                        .size(13.0)
                        .strong()
                        .color(Color32::from_white_alpha(204)),
                );
                let mut sidebar_opacity = next.sidebar_opacity;
                if ui
                    .add_sized(
                        [340.0, 20.0],
                        egui::Slider::new(&mut sidebar_opacity, 30..=100)
                            .step_by(5.0)
                            .suffix("%"),
                    )
                    .changed()
                {
                    settings_page::apply(next.to_mut(), Change::SidebarOpacity(sidebar_opacity));
                }
            });
            ui.add_space(30.0);
            {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(450.0, 340.0), Sense::hover());
                self.aurora
                    .paint_background_clipped(ui.painter(), rect, ui.max_rect());
                let preview_theme = theme::resolve(
                    next.as_ref(),
                    ui.ctx().system_theme() == Some(egui::Theme::Dark),
                );
                preview::draw_mini(
                    ui.painter(),
                    rect.shrink(12.0),
                    next.as_ref(),
                    &MiniColors::from(preview_theme),
                );
                glass::paint_noise(
                    ui.painter(),
                    rect,
                    next.noise,
                    preview_theme.dark,
                    grain,
                    ui.ctx().pixels_per_point(),
                );
            }
        });
    }

    fn step_four(&self, ui: &mut egui::Ui, theme: Theme) {
        let protections = protections::protections(protections::state());
        ui.heading(
            egui::RichText::new("Built to protect you")
                .size(32.0)
                .color(Color32::WHITE),
        );
        ui.label(
            egui::RichText::new(protections::welcome_line()).color(Color32::from_white_alpha(184)),
        );
        ui.add_space(30.0);
        let entries = [
            ProtectionKind::NoTelemetry,
            ProtectionKind::TrackerBlocking,
            ProtectionKind::HttpsOnly,
            ProtectionKind::ThirdPartyCookies,
        ];
        let tile_width = ((ui.available_width() - 12.0) / 2.0).max(150.0);
        for row in 0..2 {
            ui.horizontal(|ui| {
                for column in 0..2 {
                    let kind = entries[row * 2 + column];
                    let Some(entry) = protections.iter().find(|entry| entry.kind == kind) else {
                        continue;
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(tile_width, 145.0), Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 16.0, Color32::from_white_alpha(20));
                    ui.painter().rect_stroke(
                        rect,
                        16.0,
                        Stroke::new(1.0, Color32::from_white_alpha(32)),
                        StrokeKind::Inside,
                    );
                    let icon = Rect::from_min_size(
                        rect.min + egui::vec2(18.0, 18.0),
                        egui::vec2(32.0, 32.0),
                    );
                    if entry.active {
                        ui.painter().rect_filled(icon, 8.0, theme.accent);
                        crate::icons::shield_check(ui.painter(), icon.shrink(5.0), Color32::WHITE);
                    } else {
                        ui.painter().rect_stroke(
                            icon,
                            8.0,
                            Stroke::new(1.0, theme.accent),
                            StrokeKind::Inside,
                        );
                        crate::icons::shield(ui.painter(), icon.shrink(5.0), theme.accent);
                    }
                    ui.painter().text(
                        rect.min + egui::vec2(62.0, 22.0),
                        egui::Align2::LEFT_TOP,
                        entry.title,
                        egui::FontId::proportional(16.0),
                        if entry.active {
                            Color32::WHITE
                        } else {
                            Color32::from_white_alpha(155)
                        },
                    );
                    ui.painter().text(
                        rect.min + egui::vec2(18.0, 72.0),
                        egui::Align2::LEFT_TOP,
                        if entry.active {
                            entry.detail
                        } else {
                            "Coming in a future update"
                        },
                        egui::FontId::proportional(13.0),
                        Color32::from_white_alpha(175),
                    );
                }
            });
            ui.add_space(12.0);
        }
    }

    fn shine(&mut self, ui: &egui::Ui, card: Rect, progress: f32) {
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let width = card.width() * 0.55;
        let x = card.left() - width + eased * (card.width() + width * 2.0);
        let skew = card.height() * (-18.0_f32).to_radians().tan();
        let mut mesh = egui::Mesh::default();
        let rim = projection::rounded_rect_points(card, 34.0, 5);
        for column in 0..8 {
            let left = column as f32 / 8.0;
            let right = (column + 1) as f32 / 8.0;
            let point = |t: f32, y: f32| {
                Pos2::new(
                    x + width * t + skew * ((y - card.top()) / card.height() - 0.5),
                    y,
                )
            };
            let polygon = clip_polygon_to_rounded_rect(
                &[
                    point(left, card.top()),
                    point(right, card.top()),
                    point(right, card.bottom()),
                    point(left, card.bottom()),
                ],
                &rim,
            );
            if polygon.len() < 3 {
                continue;
            }
            let start = mesh.vertices.len() as u32;
            let left_alpha = 23.0 * (std::f32::consts::PI * left).sin().powi(2);
            let right_alpha = 23.0 * (std::f32::consts::PI * right).sin().powi(2);
            for pos in polygon {
                let t = ((pos.x - x - skew * ((pos.y - card.top()) / card.height() - 0.5)) / width
                    * 8.0
                    - column as f32)
                    .clamp(0.0, 1.0);
                mesh.vertices.push(egui::epaint::Vertex {
                    pos,
                    uv: Pos2::ZERO,
                    color: Color32::from_white_alpha(
                        (left_alpha + (right_alpha - left_alpha) * t) as u8,
                    ),
                });
            }
            for index in 2..mesh.vertices.len() as u32 - start {
                mesh.add_triangle(start, start + index - 1, start + index);
            }
        }
        ui.painter().add(mesh);
    }

    fn footer(
        &self,
        ui: &mut egui::Ui,
        card: Rect,
        time: f32,
        reduced: bool,
        advance: &mut bool,
        back: &mut bool,
    ) {
        let footer = Rect::from_min_max(
            card.left_bottom() + egui::vec2(52.0, -74.0),
            card.right_bottom() + egui::vec2(-52.0, -28.0),
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(widgets::non_negative(footer)),
            |ui| {
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(Color32::from_white_alpha(22))
                        .corner_radius(25.0)
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let pill = ui.painter().add(egui::Shape::Noop);
                                let mut labels = [Rect::NOTHING; 4];
                                for (index, label) in ["Welcome", "Setup", "Style", "Privacy"]
                                    .into_iter()
                                    .enumerate()
                                {
                                    let selected = index == self.state.index;
                                    let response = egui::Frame::new()
                                        .corner_radius(20.0)
                                        .inner_margin(egui::Margin::symmetric(12, 6))
                                        .show(ui, |ui| {
                                            ui.label(egui::RichText::new(label).color(
                                                if selected {
                                                    Color32::WHITE
                                                } else {
                                                    Color32::from_white_alpha(170)
                                                },
                                            ));
                                        });
                                    labels[index] = response.response.rect;
                                }
                                let from = self.transition.outgoing.unwrap_or(self.state.index);
                                let to = self.state.index;
                                let t = self.transition.progress.value;
                                let rect = Rect::from_min_max(
                                    labels[from].min.lerp(labels[to].min, t),
                                    labels[from].max.lerp(labels[to].max, t),
                                );
                                ui.painter().set(
                                    pill,
                                    egui::Shape::rect_filled(
                                        rect,
                                        20.0,
                                        Color32::from_white_alpha(46),
                                    ),
                                );
                            });
                        });
                });
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(widgets::non_negative(footer)),
            |ui| {
                ui.set_width(footer.width());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().button_padding = egui::vec2(28.0, 0.0);
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let label = if self.state.index == 0 {
                        "Get started"
                    } else if self.state.index == 3 {
                        "Start browsing"
                    } else {
                        "Continue"
                    };
                    let opacity = if self.state.index == 0 {
                        intro::rise(time, 1.35, 0.9, 26.0, 1.0, reduced).opacity
                    } else {
                        1.0
                    };
                    ui.set_opacity(opacity);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(label)
                                    .size(15.0)
                                    .strong()
                                    .color(Color32::from_rgb(20, 22, 30)),
                            )
                            .fill(Color32::WHITE)
                            .corner_radius(23.0)
                            .min_size(egui::vec2(160.0, 46.0)),
                        )
                        .clicked()
                    {
                        *advance = true;
                    }
                    if self.state.index > 0
                        && ui
                            .add(
                                egui::Button::new(egui::RichText::new("Back").size(15.0).strong())
                                    .fill(Color32::from_white_alpha(25))
                                    .corner_radius(23.0)
                                    .min_size(egui::vec2(0.0, 46.0)),
                            )
                            .clicked()
                    {
                        *back = true;
                    }
                });
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use eframe::egui::RawInput;

    fn screen(width: f32, height: f32) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, height))),
            ..RawInput::default()
        }
    }

    #[test]
    fn all_steps_and_setups_draw_at_window_sizes() {
        for (width, height) in [
            (2000.0, 1067.0),
            (1280.0, 800.0),
            (900.0, 600.0),
            (640.0, 480.0),
        ] {
            for setup in [
                Setup::Explorer,
                Setup::Focus,
                Setup::Power,
                Setup::Minimal,
                Setup::Classic,
            ] {
                let ctx = egui::Context::default();
                let grain = ctx.load_texture(
                    "test_grain",
                    egui::ColorImage::filled([1, 1], Color32::GRAY),
                    egui::TextureOptions::LINEAR,
                );
                let mut settings = Settings {
                    motion: aegis_core::MotionPref::Reduced,
                    ..Settings::default()
                };
                settings.apply_setup(setup);
                let mut welcome = Welcome::new(&ctx);
                for step in 0..4 {
                    welcome.state.index = step;
                    welcome.accent_picker.open = step == 2;
                    for _ in 0..3 {
                        let mut output = ctx.run_ui(screen(width, height), |ui| {
                            let theme = theme::resolve(&settings, true);
                            let _ =
                                welcome.show(ui, &settings, theme, GroupColor::Blue, true, &grain);
                        });
                        output.textures_delta.clear();
                    }
                    if step == 2 {
                        assert!(welcome.accent_picker.open);
                        assert!(welcome.accent_picker.mesh_ready());
                    }
                }
            }
        }
    }

    fn grain(ctx: &egui::Context) -> egui::TextureHandle {
        ctx.load_texture(
            "test_grain",
            egui::ColorImage::filled([1, 1], Color32::GRAY),
            egui::TextureOptions::LINEAR,
        )
    }

    fn full_motion() -> Settings {
        Settings {
            motion: aegis_core::MotionPref::Full,
            ..Settings::default()
        }
    }

    // one frame that looks 40 ms after the last one
    fn frame(
        ctx: &egui::Context,
        welcome: &mut Welcome,
        settings: &Settings,
        grain: &egui::TextureHandle,
        input: RawInput,
    ) -> ((Option<Settings>, bool), bool) {
        welcome.last_frame = Instant::now().checked_sub(std::time::Duration::from_millis(40));
        let mut result = (None, false);
        let mut output = ctx.run_ui(input, |ui| {
            let theme = theme::resolve(settings, true);
            result = welcome.show(ui, settings, theme, GroupColor::Blue, true, grain);
        });
        let uploaded = full_font_upload(&output);
        output.textures_delta.clear();
        (result, uploaded)
    }

    // whether the whole font atlas was sent again
    fn full_font_upload(output: &egui::FullOutput) -> bool {
        output.textures_delta.set.iter().any(|(id, deltas)| {
            *id == egui::TextureId::default() && deltas.iter().any(|delta| delta.pos.is_none())
        })
    }

    fn press() -> egui::Event {
        egui::Event::PointerButton {
            pos: Pos2::new(40.0, 40.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn opening_frames_draw_at_window_sizes() {
        for (width, height) in [
            (1280.0, 800.0),
            (640.0, 480.0),
            (300.0, 200.0),
            (60.0, 40.0),
        ] {
            for clock in [0.0, 0.3, 0.9, 1.2, 1.5, 1.8, 2.2, 2.65, 3.0] {
                let ctx = egui::Context::default();
                let grain = grain(&ctx);
                let settings = full_motion();
                let mut welcome = Welcome::new(&ctx);
                welcome.clock = clock;
                let _ = frame(&ctx, &mut welcome, &settings, &grain, screen(width, height));
                assert!(welcome.content >= 0.0);
            }
        }
    }

    #[test]
    fn clock_starts_on_the_first_frame_and_clamps_hitches() {
        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = full_motion();
        let mut welcome = Welcome::new(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(60));
        let mut output = ctx.run_ui(screen(900.0, 600.0), |ui| {
            let theme = theme::resolve(&settings, true);
            let _ = welcome.show(ui, &settings, theme, GroupColor::Blue, true, &grain);
        });
        output.textures_delta.clear();
        assert_eq!(welcome.clock, 0.0);
        let _ = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
        assert!((welcome.clock - 1.0 / 30.0).abs() < 0.0001);
    }

    #[test]
    fn content_waits_for_the_opening_then_runs() {
        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = full_motion();
        let mut welcome = Welcome::new(&ctx);
        let mut last = 0.0;
        for step in 0..150 {
            let _ = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
            assert!(welcome.content >= last);
            if welcome.clock < 1.45 {
                assert_eq!(welcome.content, 0.0, "step {step}");
            }
            last = welcome.content;
        }
        assert!(welcome.opening.is_none());
        assert!((welcome.content - (welcome.clock - 1.45)).abs() < 0.001);
    }

    #[test]
    fn press_skips_the_opening_without_advancing() {
        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = full_motion();
        let mut welcome = Welcome::new(&ctx);
        let _ = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
        for _ in 0..6 {
            let _ = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
        }
        assert!(welcome.clock < 1.0);
        let mut input = screen(900.0, 600.0);
        input.events.push(press());
        let (result, _) = frame(&ctx, &mut welcome, &settings, &grain, input);
        assert_eq!(result, (None, false));
        assert!(welcome.opening.as_ref().is_some_and(|open| open.skipping()));
        let mut last = welcome.content;
        for _ in 0..8 {
            let (result, _) = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
            assert!(!result.1);
            assert!(welcome.content >= last);
            last = welcome.content;
        }
        assert!(welcome.opening.is_none());
        assert!(welcome.content >= 3.0);
        assert_eq!(welcome.state.index, 0);
    }

    #[test]
    fn reduced_motion_has_no_opening() {
        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = Settings {
            motion: aegis_core::MotionPref::Reduced,
            ..Settings::default()
        };
        let mut welcome = Welcome::new(&ctx);
        assert!(welcome.opening.is_some());
        let _ = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
        assert!(welcome.opening.is_none());
        assert_eq!(welcome.content, 0.0);
        let pose = intro::rise(0.0, 0.55, 0.9, 26.0, 1.0, true);
        assert_eq!((pose.opacity, pose.offset), (1.0, 0.0));
    }

    #[test]
    fn eyebrow_rect_follows_the_card() {
        let card = Rect::from_min_size(Pos2::new(40.0, 30.0), egui::vec2(1000.0, 560.0));
        let rect = eyebrow_logo_rect(card);
        assert_eq!(rect.size(), egui::vec2(18.0, 18.0));
        assert_eq!(rect.min.x, 92.0);
        assert_eq!(
            eyebrow_logo_rect(card.translate(egui::vec2(5.0, 7.0)))
                .min
                .y,
            rect.min.y + 7.0
        );
    }

    #[test]
    fn shine_polygon_stays_inside_rounded_card() {
        let card = Rect::from_min_size(Pos2::new(40.0, 30.0), egui::vec2(200.0, 120.0));
        let rim = projection::rounded_rect_points(card, 34.0, 5);
        let polygon = clip_polygon_to_rounded_rect(
            &[
                card.left_top(),
                card.right_top(),
                card.right_bottom(),
                card.left_bottom(),
            ],
            &rim,
        );
        assert!(polygon.len() >= 4);
        assert!(!polygon.contains(&card.right_top()));
        for point in polygon {
            for index in 0..rim.len() {
                let a = rim[index];
                let b = rim[(index + 1) % rim.len()];
                let side = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
                assert!(side >= -0.001, "point {point:?} outside edge {index}");
            }
        }
    }

    #[test]
    fn welcome_is_built_only_when_due() {
        assert!(should_show(false, false));
        assert!(should_show(true, true));
        assert!(!should_show(true, false));
        let ctx = egui::Context::default();
        let before = ctx.tex_manager().read().num_allocated();
        let mut slot: Option<Welcome> = None;
        if should_show(true, false) {
            slot.get_or_insert_with(|| Welcome::new(&ctx));
        }
        assert!(slot.is_none());
        assert_eq!(ctx.tex_manager().read().num_allocated(), before);
    }

    #[test]
    fn finishing_drops_the_welcome_and_its_textures() {
        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = full_motion();
        let base = ctx.tex_manager().read().num_allocated();
        let mut slot = Some(Welcome::new(&ctx));
        assert_eq!(ctx.tex_manager().read().num_allocated(), base + 2);
        if let Some(welcome) = slot.as_mut() {
            let _ = frame(&ctx, welcome, &settings, &grain, screen(900.0, 600.0));
        }
        ctx.memory_mut(|memory| memory.data.insert_temp(root_id(), 42_u32));
        finish(&mut slot, &ctx);
        assert!(slot.is_none());
        assert!(ctx.memory(|memory| memory.data.get_temp::<u32>(root_id()).is_none()));
        let after = ctx.tex_manager().read().num_allocated();
        assert_eq!(after, base);
        finish(&mut slot, &ctx);
    }

    #[test]
    fn finishing_rebuilds_the_font_atlas_in_one_frame() {
        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = full_motion();
        let mut welcome = Welcome::new(&ctx);
        let mut uploaded = true;
        for _ in 0..5 {
            (_, uploaded) = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
        }
        assert!(!uploaded);
        welcome.release(&ctx);
        let mut uploads = 0;
        for _ in 0..3 {
            let mut output = ctx.run_ui(screen(900.0, 600.0), |ui| {
                ui.label("Browsing");
            });
            if full_font_upload(&output) {
                uploads += 1;
            }
            output.textures_delta.clear();
        }
        assert_eq!(uploads, 1);
    }

    #[test]
    fn font_atlas_returns_to_browsing_size() {
        let normal = egui::Context::default();
        let mut output = normal.run_ui(screen(900.0, 600.0), |ui| {
            ui.label("Browsing");
        });
        output.textures_delta.clear();
        let normal_size = normal.fonts(|fonts| fonts.font_image_size());

        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = full_motion();
        let mut welcome = Welcome::new(&ctx);
        for _ in 0..5 {
            let _ = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
        }
        welcome.release(&ctx);
        let mut output = ctx.run_ui(screen(900.0, 600.0), |ui| {
            ui.label("Browsing");
        });
        output.textures_delta.clear();
        assert_eq!(ctx.fonts(|fonts| fonts.font_image_size()), normal_size);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_finish_uses_the_current_setup() {
        let ctx = egui::Context::default();
        let grain = grain(&ctx);
        let settings = full_motion();
        let mut welcome = Welcome::new(&ctx);
        welcome.finish_at = Some(0.0);
        let (result, _) = frame(&ctx, &mut welcome, &settings, &grain, screen(900.0, 600.0));
        let (done, finished) = result;
        assert!(finished);
        let done = done.unwrap_or_default();
        assert!(done.welcome_done);
        assert_eq!(done.setup, settings.setup);
    }

    #[test]
    fn step_bounds_and_finish() {
        let mut state = StepState::default();
        let mut settings = Settings::default();
        state.back();
        assert_eq!(state.index, 0);
        for index in 1..=3 {
            assert!(!state.next(&mut settings));
            assert_eq!(state.index, index);
            assert!(!settings.welcome_done);
        }
        assert!(state.next(&mut settings));
        assert_eq!(state.index, 3);
        assert!(settings.welcome_done);
        state.back();
        assert_eq!(state.index, 2);
    }

    #[test]
    fn transition_retargets_and_drops_outgoing_page() {
        let mut transition = Transition::new();
        let mut state = StepState::default();
        let mut settings = Settings::default();
        let from = state.index;
        assert!(!state.next(&mut settings));
        transition.start(from, state.index);
        transition.frame(1.0 / 60.0, false);
        let from = state.index;
        assert!(!state.next(&mut settings));
        transition.start(from, state.index);
        assert_eq!(transition.outgoing, Some(1));
        let from = state.index;
        state.back();
        transition.start(from, state.index);
        assert_eq!(transition.direction, -1.0);
        for _ in 0..120 {
            transition.frame(1.0 / 60.0, false);
        }
        assert_eq!(transition.outgoing, None);
        assert_eq!(transition.progress.value, 1.0);
    }

    #[test]
    fn outgoing_page_fades_before_incoming_page() {
        assert_eq!(outgoing_opacity(0.0), 1.0);
        assert!(outgoing_opacity(0.4) < 0.05);
        assert_eq!(outgoing_opacity(1.0), 0.0);
    }
}
