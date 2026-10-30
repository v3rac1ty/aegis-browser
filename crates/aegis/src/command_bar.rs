use std::time::{Duration, Instant};

use aegis_core::SearchEngine;
use eframe::egui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    NewTab,
    CurrentTab,
}

pub fn classify_input(input: &str, search: SearchEngine) -> String {
    let value = input.trim();
    if let Ok(url) = aegis_ipc::page_url(value) {
        return url.into_string();
    }
    if !value.contains(':')
        && value.contains('.')
        && !value.contains(char::is_whitespace)
        && let Ok(url) = aegis_ipc::page_url(&format!("https://{value}"))
    {
        return url.into_string();
    }
    search.query_url(value)
}

pub struct CommandBar {
    pub mode: Option<Mode>,
    text: String,
    opened_at: Instant,
    focused_frames: u8,
}

impl CommandBar {
    pub fn new() -> Self {
        Self {
            mode: None,
            text: String::new(),
            opened_at: Instant::now(),
            focused_frames: 0,
        }
    }

    pub fn open(&mut self, mode: Mode) {
        self.mode = Some(mode);
        self.text.clear();
        self.opened_at = Instant::now();
        self.focused_frames = 0;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        content: egui::Rect,
        window: egui::Rect,
    ) -> Option<(Mode, String)> {
        let mode = self.mode?;
        let width = 600.0_f32.min(content.width());
        let position = egui::pos2(
            window.min.x + content.center().x - width / 2.0,
            window.min.y + content.min.y + 80.0,
        );
        let builder = egui::ViewportBuilder::default()
            .with_position(position)
            .with_inner_size([width, 56.0])
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false);
        let (focused, key_down, enter, escape) = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("command_bar"),
            builder,
            |ui, _| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.add_space(7.0);
                    let response = ui.add_sized(
                        [ui.available_width(), 36.0],
                        egui::TextEdit::singleline(&mut self.text).hint_text("Enter URL or search"),
                    );
                    if self.focused_frames == 0 {
                        response.request_focus();
                    }
                });
                ui.input(|input| {
                    (
                        input.viewport().focused == Some(true),
                        input.key_down(egui::Key::L),
                        input.key_pressed(egui::Key::Enter),
                        input.key_pressed(egui::Key::Escape),
                    )
                })
            },
        );
        if escape {
            self.mode = None;
        } else if enter {
            self.mode = None;
            return Some((mode, self.text.clone()));
        } else if focused && !key_down {
            self.focused_frames = self.focused_frames.saturating_add(1);
        } else if !focused && self.opened_at.elapsed() > Duration::from_millis(500) {
            self.mode = None;
        }
        if self.mode.is_some() && !focused {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of("command_bar"),
                egui::ViewportCommand::Focus,
            );
        }
        ctx.request_repaint_of(egui::ViewportId::ROOT);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::classify_input;
    use aegis_core::SearchEngine;

    #[test]
    fn classifies_input() {
        let search = SearchEngine::DuckDuckGo;
        assert_eq!(
            classify_input("https://example.com", search),
            "https://example.com/"
        );
        assert_eq!(
            classify_input("example.com", search),
            "https://example.com/"
        );
        assert_eq!(
            classify_input("two words", search),
            "https://duckduckgo.com/?q=two%20words"
        );
        assert_eq!(
            classify_input("javascript:alert(1)", search),
            "https://duckduckgo.com/?q=javascript%3Aalert%281%29"
        );
    }
}
