use std::time::{Duration, Instant};

use aegis_core::{Browser, SearchEngine, TabId};
use eframe::egui::{self, Color32, FontId, Stroke};

use crate::matcher::{self, TabCandidate};
use crate::sidebar::avatar;
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

fn primary_url(input: &str, search: SearchEngine) -> (String, bool) {
    let value = input.trim();
    let is_url = aegis_ipc::page_url(value).is_ok()
        || (!value.contains(':')
            && value.contains('.')
            && !value.contains(char::is_whitespace)
            && aegis_ipc::page_url(&format!("https://{value}")).is_ok());
    (classify_input(value, search), is_url)
}

pub fn search_engine_name(search: SearchEngine) -> &'static str {
    match search {
        SearchEngine::DuckDuckGo => "DuckDuckGo",
        SearchEngine::Brave => "Brave",
        SearchEngine::Startpage => "Startpage",
        SearchEngine::Google => "Google",
        SearchEngine::Bing => "Bing",
    }
}

pub fn search_engine_home_url(search: SearchEngine) -> &'static str {
    match search {
        SearchEngine::DuckDuckGo => "https://duckduckgo.com",
        SearchEngine::Brave => "https://search.brave.com",
        SearchEngine::Startpage => "https://www.startpage.com",
        SearchEngine::Google => "https://www.google.com",
        SearchEngine::Bing => "https://www.bing.com",
    }
}

#[derive(Clone, Debug)]
pub struct TabEntry {
    id: TabId,
    title: String,
    host: String,
    workspace: String,
    current_workspace: bool,
    active: bool,
    last_active_ms: u64,
}

impl TabEntry {
    fn from_browser(browser: &Browser) -> Vec<Self> {
        let mut entries = Vec::new();
        for workspace in browser.workspaces() {
            for id in browser.visual_order(workspace.id()) {
                let Some(tab) = browser.tab(id) else {
                    continue;
                };
                let host = aegis_ipc::page_url(tab.url()).map_or_else(
                    |_| tab.url().to_owned(),
                    |url| aegis_ipc::display_host(&url),
                );
                entries.push(Self {
                    id,
                    title: tab.title().to_owned(),
                    host,
                    workspace: workspace.name().to_owned(),
                    current_workspace: workspace.id() == browser.active_workspace(),
                    active: browser.active_tab() == Some(id),
                    last_active_ms: tab.last_active_ms(),
                });
            }
        }
        entries
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuggestionAction {
    Open(String),
    ActivateTab(TabId),
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub action: SuggestionAction,
    pub title: String,
    pub secondary: String,
    pub avatar: String,
}

pub fn suggestions(
    input: &str,
    mode: Mode,
    search: SearchEngine,
    tabs: &[TabEntry],
) -> Vec<Suggestion> {
    let query = input.trim();
    if query.is_empty() && mode == Mode::CurrentTab {
        return Vec::new();
    }
    let mut result = Vec::new();
    if !query.is_empty() {
        let (url, is_url) = primary_url(query, search);
        result.push(Suggestion {
            action: SuggestionAction::Open(url.clone()),
            title: if is_url {
                format!("Open {url}")
            } else {
                format!("Search {} for \"{query}\"", search_engine_name(search))
            },
            secondary: if is_url {
                url.clone()
            } else {
                "Search".to_owned()
            },
            avatar: if is_url { url } else { "Search".to_owned() },
        });
    }
    let mut eligible = Vec::new();
    for tab in tabs {
        if mode == Mode::CurrentTab && tab.active {
            continue;
        }
        eligible.push(tab);
    }
    let mut ranked = if query.is_empty() {
        let mut indices: Vec<usize> = (0..eligible.len()).collect();
        indices.sort_by_key(|index| std::cmp::Reverse(eligible[*index].last_active_ms));
        indices.truncate(6);
        indices
    } else {
        let current_indices: Vec<usize> = eligible
            .iter()
            .enumerate()
            .filter_map(|(index, tab)| tab.current_workspace.then_some(index))
            .collect();
        let current: Vec<TabCandidate<'_>> = current_indices
            .iter()
            .map(|index| {
                let tab = eligible[*index];
                TabCandidate {
                    title: &tab.title,
                    display_host: &tab.host,
                    last_active_ms: tab.last_active_ms,
                }
            })
            .collect();
        let current_matches = matcher::rank_tabs(query, &current);
        if current_matches.len() >= 3 {
            current_matches
                .into_iter()
                .map(|(index, _)| current_indices[index])
                .collect()
        } else {
            let candidates: Vec<TabCandidate<'_>> = eligible
                .iter()
                .map(|tab| TabCandidate {
                    title: &tab.title,
                    display_host: &tab.host,
                    last_active_ms: tab.last_active_ms,
                })
                .collect();
            matcher::rank_tabs(query, &candidates)
                .into_iter()
                .map(|(index, _)| index)
                .collect()
        }
    };
    ranked.truncate(6);
    for index in ranked {
        let tab = eligible[index];
        result.push(Suggestion {
            action: SuggestionAction::ActivateTab(tab.id),
            title: if tab.title.is_empty() {
                tab.host.clone()
            } else {
                tab.title.clone()
            },
            secondary: if tab.current_workspace {
                tab.host.clone()
            } else {
                format!("{} · {}", tab.host, tab.workspace)
            },
            avatar: tab.host.clone(),
        });
    }
    result
}

#[derive(Default)]
pub struct Selection {
    index: usize,
}

impl Selection {
    pub fn up(&mut self, len: usize) {
        if len > 0 {
            self.index = (self.index + len - 1) % len;
        }
    }

    pub fn down(&mut self, len: usize) {
        if len > 0 {
            self.index = (self.index + 1) % len;
        }
    }

    pub fn reset(&mut self) {
        self.index = 0;
    }

    pub fn index(&self) -> usize {
        self.index
    }
}

pub struct BarAction {
    pub mode: Mode,
    pub suggestion: SuggestionAction,
    pub force_new_tab: bool,
}

struct BarKeys {
    focused: bool,
    key_down: bool,
    enter: bool,
    escape: bool,
    force_new_tab: bool,
    up: bool,
    down: bool,
}

struct ViewportResult {
    keys: BarKeys,
    clicked: Option<usize>,
}

pub struct CommandBar {
    pub mode: Option<Mode>,
    text: String,
    opened_at: Instant,
    focused_frames: u8,
    selection: Selection,
}

impl CommandBar {
    pub fn new() -> Self {
        Self {
            mode: None,
            text: String::new(),
            opened_at: Instant::now(),
            focused_frames: 0,
            selection: Selection::default(),
        }
    }

    pub fn open(&mut self, mode: Mode, active_url: Option<&str>) {
        self.mode = Some(mode);
        self.text = if mode == Mode::CurrentTab {
            active_url.unwrap_or_default().to_owned()
        } else {
            String::new()
        };
        self.opened_at = Instant::now();
        self.focused_frames = 0;
        self.selection.reset();
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        content: egui::Rect,
        window: egui::Rect,
        browser: &Browser,
    ) -> Option<BarAction> {
        let mode = self.mode?;
        let entries = TabEntry::from_browser(browser);
        let mut rows = suggestions(&self.text, mode, browser.settings().search_engine, &entries);
        let width = 640.0_f32.min((content.width() - 48.0).max(1.0));
        let height = 44.0 + 36.0 * rows.len() as f32 + 2.0;
        let position = egui::pos2(
            window.min.x + content.center().x - width / 2.0,
            window.min.y + content.min.y + 72.0,
        );
        let builder = egui::ViewportBuilder::default()
            .with_position(position)
            .with_inner_size([width, height])
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(false);
        let result = ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("command_bar"),
            builder,
            |ui, _| {
                let palette = theme::palette(ui.ctx());
                let mut clicked = None;
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        egui::Frame::NONE
                            .fill(palette.background)
                            .corner_radius(12.0)
                            .stroke(Stroke::new(1.0, palette.border))
                            .show(ui, |ui| {
                                ui.set_width(width - 2.0);
                                ui.horizontal(|ui| {
                                    ui.set_min_height(33.0);
                                    ui.add_space(12.0);
                                    let output = egui::TextEdit::singleline(&mut self.text)
                                        .hint_text("Enter URL or search")
                                        .font(FontId::proportional(15.0))
                                        .frame(egui::Frame::NONE)
                                        .desired_width(width - 28.0)
                                        .show(ui);
                                    if self.focused_frames == 0 {
                                        output.response.request_focus();
                                        if mode == Mode::CurrentTab {
                                            let mut state = output.state;
                                            state.cursor.set_char_range(Some(
                                                egui::text::CCursorRange::select_all_str(
                                                    &self.text,
                                                ),
                                            ));
                                            state.store(ui.ctx(), output.response.id);
                                        }
                                    }
                                    if output.response.changed() {
                                        self.selection.reset();
                                        rows = suggestions(
                                            &self.text,
                                            mode,
                                            browser.settings().search_engine,
                                            &entries,
                                        );
                                    }
                                });
                                ui.add_space(10.0);
                                let separator = ui.allocate_space(egui::vec2(width - 2.0, 1.0)).1;
                                ui.painter().line_segment(
                                    [separator.left_top(), separator.right_top()],
                                    Stroke::new(1.0, palette.separator),
                                );
                                let pointer_moved =
                                    ui.input(|input| input.pointer.delta().length_sq() > 0.0);
                                for (index, row) in rows.iter().enumerate() {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(width - 2.0, 36.0),
                                        egui::Sense::click(),
                                    );
                                    if response.hovered() && pointer_moved {
                                        self.selection.index = index;
                                    }
                                    if response.clicked() {
                                        clicked = Some(index);
                                    }
                                    if self.selection.index == index {
                                        ui.painter().rect_filled(
                                            rect.shrink2(egui::vec2(4.0, 1.0)),
                                            6.0,
                                            palette.active,
                                        );
                                    }
                                    let avatar_rect = egui::Rect::from_min_size(
                                        rect.min + egui::vec2(12.0, 7.0),
                                        egui::vec2(22.0, 22.0),
                                    );
                                    let (letter, color) = avatar(&row.avatar);
                                    const COLORS: [Color32; 8] = [
                                        Color32::from_rgb(101, 132, 184),
                                        Color32::from_rgb(168, 112, 138),
                                        Color32::from_rgb(106, 156, 130),
                                        Color32::from_rgb(167, 137, 88),
                                        Color32::from_rgb(130, 114, 174),
                                        Color32::from_rgb(95, 151, 163),
                                        Color32::from_rgb(171, 117, 100),
                                        Color32::from_rgb(121, 143, 105),
                                    ];
                                    ui.painter().rect_filled(avatar_rect, 5.0, COLORS[color]);
                                    ui.painter().text(
                                        avatar_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        letter,
                                        FontId::proportional(12.0),
                                        Color32::WHITE,
                                    );
                                    let hint =
                                        if matches!(row.action, SuggestionAction::ActivateTab(_)) {
                                            "Switch to tab"
                                        } else {
                                            "↵"
                                        };
                                    ui.painter().text(
                                        rect.right_center() - egui::vec2(12.0, 0.0),
                                        egui::Align2::RIGHT_CENTER,
                                        hint,
                                        FontId::proportional(11.0),
                                        palette.secondary,
                                    );
                                    let text_rect = egui::Rect::from_min_max(
                                        rect.min + egui::vec2(42.0, 2.0),
                                        rect.max - egui::vec2(92.0, 2.0),
                                    );
                                    let painter = ui.painter().with_clip_rect(text_rect);
                                    painter.text(
                                        text_rect.min + egui::vec2(0.0, 1.0),
                                        egui::Align2::LEFT_TOP,
                                        &row.title,
                                        FontId::proportional(12.0),
                                        palette.text,
                                    );
                                    painter.text(
                                        text_rect.min + egui::vec2(0.0, 17.0),
                                        egui::Align2::LEFT_TOP,
                                        &row.secondary,
                                        FontId::proportional(10.0),
                                        palette.secondary,
                                    );
                                }
                            });
                    });
                let keys = ui.input(|input| BarKeys {
                    focused: input.viewport().focused == Some(true),
                    key_down: input.key_down(egui::Key::L),
                    enter: input.key_pressed(egui::Key::Enter),
                    escape: input.key_pressed(egui::Key::Escape),
                    force_new_tab: input.modifiers.command,
                    up: input.key_pressed(egui::Key::ArrowUp),
                    down: input.key_pressed(egui::Key::ArrowDown),
                });
                if keys.up {
                    self.selection.up(rows.len());
                }
                if keys.down {
                    self.selection.down(rows.len());
                }
                ViewportResult { keys, clicked }
            },
        );
        if result.keys.escape {
            self.mode = None;
        } else if result.keys.enter || result.clicked.is_some() {
            let index = result.clicked.unwrap_or(self.selection.index());
            if let Some(row) = rows.get(index) {
                self.mode = None;
                return Some(BarAction {
                    mode,
                    suggestion: row.action.clone(),
                    force_new_tab: result.keys.force_new_tab,
                });
            }
        } else if result.keys.focused && !result.keys.key_down {
            self.focused_frames = self.focused_frames.saturating_add(1);
        } else if !result.keys.focused && self.opened_at.elapsed() > Duration::from_millis(500) {
            self.mode = None;
        }
        if self.mode.is_some() && !result.keys.focused {
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
    use super::*;

    fn tab(
        id: u64,
        title: &str,
        workspace: &str,
        current: bool,
        active: bool,
        time: u64,
    ) -> TabEntry {
        TabEntry {
            id: TabId::from_raw(id),
            title: title.to_owned(),
            host: "example.com".to_owned(),
            workspace: workspace.to_owned(),
            current_workspace: current,
            active,
            last_active_ms: time,
        }
    }

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

    #[test]
    fn suggestions_in_both_modes() {
        let tabs = [
            tab(1, "Example", "Home", true, true, 3),
            tab(2, "Examples", "Home", true, false, 2),
        ];
        let new = suggestions("Example", Mode::NewTab, SearchEngine::DuckDuckGo, &tabs);
        assert!(matches!(new[0].action, SuggestionAction::Open(_)));
        assert_eq!(new.len(), 3);
        let current = suggestions("Example", Mode::CurrentTab, SearchEngine::DuckDuckGo, &tabs);
        assert_eq!(current.len(), 2);
        assert_eq!(
            current[1].action,
            SuggestionAction::ActivateTab(TabId::from_raw(2))
        );
    }

    #[test]
    fn empty_input_and_cross_workspace() {
        let tabs = [
            tab(1, "Alpha", "Home", true, true, 1),
            tab(2, "Alpha", "Work", false, false, 3),
        ];
        assert!(suggestions("", Mode::CurrentTab, SearchEngine::DuckDuckGo, &tabs).is_empty());
        let recent = suggestions("", Mode::NewTab, SearchEngine::DuckDuckGo, &tabs);
        assert_eq!(
            recent[0].action,
            SuggestionAction::ActivateTab(TabId::from_raw(2))
        );
        let matches = suggestions("Alpha", Mode::NewTab, SearchEngine::DuckDuckGo, &tabs);
        assert!(matches[1].secondary.contains("Work"));
    }

    #[test]
    fn three_current_matches_keep_other_workspaces_out() {
        let tabs = [
            tab(1, "Alpha", "Home", true, false, 1),
            tab(2, "Alpha", "Home", true, false, 2),
            tab(3, "Alpha", "Home", true, false, 3),
            tab(4, "Alpha", "Work", false, false, 100),
        ];
        let matches = suggestions("Alpha", Mode::NewTab, SearchEngine::DuckDuckGo, &tabs);
        assert_eq!(matches.len(), 4);
        assert!(matches.iter().all(|row| !row.secondary.contains("Work")));
    }

    #[test]
    fn selection_wraps_and_resets() {
        let mut selection = Selection::default();
        selection.up(3);
        assert_eq!(selection.index(), 2);
        selection.down(3);
        assert_eq!(selection.index(), 0);
        selection.down(3);
        selection.reset();
        assert_eq!(selection.index(), 0);
    }
}
