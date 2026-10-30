use aegis_core::{
    Browser, Command, Group, GroupColor, GroupId, Item, Location, SearchEngine, TabId,
};
use eframe::egui::{self, Color32, FontId, Sense, Stroke, StrokeKind};

use crate::command_bar::{self, Mode};
use crate::theme::{self, Palette};

pub enum Action {
    Command(Command),
    OpenBar(Mode),
    Back,
    Forward,
    Reload,
    Stop,
    DismissNotice,
    SetCompact(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Pinned(usize),
    Item(usize),
    Group(GroupId, usize),
}

pub fn drop_location(target: DropTarget, lower_half: bool) -> Location {
    match target {
        DropTarget::Pinned(index) => Location::Pinned(index + usize::from(lower_half)),
        DropTarget::Item(index) => Location::Items(index + usize::from(lower_half)),
        DropTarget::Group(id, index) => Location::Group(id, index + usize::from(lower_half)),
    }
}

pub fn location_after_removal(browser: &Browser, tab: TabId, to: Location) -> Location {
    let Some(workspace) = browser.workspace(browser.active_workspace()) else {
        return to;
    };
    match to {
        Location::Pinned(mut index) => {
            if let Some(source) = workspace.pinned().iter().position(|id| *id == tab)
                && source < index
            {
                index -= 1;
            }
            Location::Pinned(index)
        }
        Location::Items(mut index) => {
            for (source, item) in workspace.items().iter().enumerate() {
                let removed = match item {
                    Item::Tab(id) => *id == tab,
                    Item::Group(group) => group.tabs().len() == 1 && group.tabs().contains(&tab),
                };
                if removed && source < index {
                    index -= 1;
                    break;
                }
            }
            Location::Items(index)
        }
        Location::Group(group_id, mut index) => {
            for item in workspace.items() {
                if let Item::Group(group) = item
                    && group.id() == group_id
                    && let Some(source) = group.tabs().iter().position(|id| *id == tab)
                    && source < index
                {
                    index -= 1;
                }
            }
            Location::Group(group_id, index)
        }
    }
}

pub fn avatar(host: &str) -> (char, usize) {
    let letter = host
        .chars()
        .find(|ch| ch.is_alphanumeric())
        .unwrap_or('?')
        .to_ascii_uppercase();
    let mut hash = 2166136261_u32;
    for byte in host.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
    }
    (letter, (hash as usize) % AVATAR_COLORS.len())
}

const AVATAR_COLORS: [Color32; 8] = [
    Color32::from_rgb(101, 132, 184),
    Color32::from_rgb(168, 112, 138),
    Color32::from_rgb(106, 156, 130),
    Color32::from_rgb(167, 137, 88),
    Color32::from_rgb(130, 114, 174),
    Color32::from_rgb(95, 151, 163),
    Color32::from_rgb(171, 117, 100),
    Color32::from_rgb(121, 143, 105),
];

#[derive(Clone, Copy)]
enum Menu {
    Tab(TabId, egui::Pos2),
    Group(GroupId, egui::Pos2),
}

pub struct Sidebar {
    menu: Option<Menu>,
    rename: Option<(GroupId, String)>,
    settings_open: bool,
}

impl Sidebar {
    pub fn new() -> Self {
        Self {
            menu: None,
            rename: None,
            settings_open: false,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        loading: bool,
        notice: Option<&str>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let palette = theme::palette(ui.ctx());
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, palette.background);
        ui.set_width(ui.available_width());
        ui.add_space(theme::PADDING);
        self.top_bar(ui, browser, loading, palette, &mut actions);
        ui.add_space(12.0);
        let Some(workspace) = browser.workspace(browser.active_workspace()) else {
            return actions;
        };
        self.pinned(ui, browser, workspace.pinned(), palette, &mut actions);
        ui.add_space(10.0);
        let rect = ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, palette.separator),
        );
        ui.add_space(8.0);
        let footer_height = if notice.is_some() { 94.0 } else { 68.0 };
        let list_height = (ui.available_height() - footer_height).max(40.0);
        egui::ScrollArea::vertical()
            .max_height(list_height)
            .min_scrolled_height(list_height)
            .show(ui, |ui| {
                for (index, item) in workspace.items().iter().enumerate() {
                    match item {
                        Item::Tab(id) => self.tab_row(
                            ui,
                            browser,
                            *id,
                            DropTarget::Item(index),
                            0.0,
                            palette,
                            &mut actions,
                        ),
                        Item::Group(group) => {
                            self.group(ui, browser, group, index, palette, &mut actions)
                        }
                    }
                }
                let response = ui.add_sized(
                    [ui.available_width(), theme::ROW_HEIGHT],
                    egui::Button::new(
                        egui::RichText::new("+ New Tab")
                            .size(13.0)
                            .color(palette.secondary),
                    )
                    .frame(false),
                );
                if response.clicked() {
                    actions.push(Action::OpenBar(Mode::NewTab));
                }
                self.drop(
                    ui,
                    &response,
                    DropTarget::Item(workspace.items().len()),
                    palette,
                    &mut actions,
                );
            });
        ui.add_space(8.0);
        let mut gear_rect = egui::Rect::NOTHING;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(workspace.name())
                    .size(11.0)
                    .color(palette.secondary),
            );
            ui.add_space((ui.available_width() - 28.0).max(0.0));
            let gear = ui.add_sized([24.0, 24.0], egui::Button::new("⚙").frame(false));
            gear_rect = gear.rect;
            if gear.clicked() {
                self.settings_open = !self.settings_open;
            }
        });
        if let Some(notice) = notice {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(notice)
                        .size(11.0)
                        .color(palette.secondary),
                );
                if ui.small_button("×").clicked() {
                    actions.push(Action::DismissNotice);
                }
            });
        }
        self.menu(ui, browser, palette, &mut actions);
        self.settings(ui, browser, palette, gear_rect, &mut actions);
        actions
    }

    fn settings(
        &mut self,
        ui: &egui::Ui,
        browser: &Browser,
        palette: Palette,
        gear_rect: egui::Rect,
        actions: &mut Vec<Action>,
    ) {
        if !self.settings_open {
            return;
        }
        let sidebar = ui.max_rect();
        let width = 220.0_f32.min(sidebar.width() - 8.0);
        let x = (gear_rect.right() - width).max(sidebar.left() + 4.0);
        let y = (gear_rect.top() - 246.0).max(sidebar.top() + 4.0);
        let response = egui::Area::new(egui::Id::unique("sidebar_settings"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x, y))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(palette.background)
                    .show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        ui.label(egui::RichText::new("Search engine").size(12.0).strong());
                        let current = browser.settings().search_engine;
                        for engine in [
                            SearchEngine::DuckDuckGo,
                            SearchEngine::Brave,
                            SearchEngine::Startpage,
                            SearchEngine::Google,
                            SearchEngine::Bing,
                        ] {
                            if ui
                                .radio(current == engine, command_bar::search_engine_name(engine))
                                .clicked()
                            {
                                let mut settings = browser.settings().clone();
                                settings.search_engine = engine;
                                actions.push(Action::Command(Command::SetSettings(settings)));
                            }
                        }
                        ui.separator();
                        let mut compact = browser.settings().compact;
                        if ui.checkbox(&mut compact, "Compact mode").changed() {
                            actions.push(Action::SetCompact(compact));
                        }
                        ui.label(
                            egui::RichText::new("Hide sidebar until hovered")
                                .size(10.0)
                                .color(palette.secondary),
                        );
                    });
            });
        let close = ui.input(|input| {
            input.key_pressed(egui::Key::Escape)
                || (input.pointer.any_pressed()
                    && input.pointer.interact_pos().is_some_and(|pos| {
                        !response.response.rect.contains(pos) && !gear_rect.contains(pos)
                    }))
        });
        if close {
            self.settings_open = false;
        }
    }

    fn top_bar(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        loading: bool,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let back_enabled = browser
                .active_tab()
                .and_then(|id| browser.tab(id))
                .is_some_and(|tab| tab.history().len() > 1 || tab.opener().is_some());
            let view_enabled = browser.active_tab().is_some();
            if ui
                .add_enabled_ui(back_enabled, |ui| {
                    ui.add_sized([20.0, 20.0], egui::Button::new("←").frame(false))
                })
                .inner
                .clicked()
            {
                actions.push(Action::Back);
            }
            if ui
                .add_enabled_ui(view_enabled, |ui| {
                    ui.add_sized([20.0, 20.0], egui::Button::new("→").frame(false))
                })
                .inner
                .clicked()
            {
                actions.push(Action::Forward);
            }
            if ui
                .add_enabled_ui(view_enabled, |ui| {
                    ui.add_sized(
                        [20.0, 20.0],
                        egui::Button::new(if loading { "×" } else { "↻" }).frame(false),
                    )
                })
                .inner
                .clicked()
            {
                actions.push(if loading {
                    Action::Stop
                } else {
                    Action::Reload
                });
            }
            let host = browser
                .active_tab()
                .and_then(|id| browser.tab(id))
                .map_or_else(|| "New Tab".to_owned(), |tab| display_host(tab.url()));
            let pill = egui::Button::new(
                egui::RichText::new(host)
                    .size(11.0)
                    .color(palette.secondary),
            )
            .fill(palette.hover)
            .corner_radius(10.0);
            if ui.add_sized([ui.available_width(), 24.0], pill).clicked() {
                actions.push(Action::OpenBar(Mode::CurrentTab));
            }
        });
    }

    fn pinned(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        pinned: &[TabId],
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        for chunk in pinned.chunks(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (column, id) in chunk.iter().enumerate() {
                    let Some(tab) = browser.tab(*id) else {
                        continue;
                    };
                    let index = pinned
                        .iter()
                        .position(|candidate| candidate == id)
                        .unwrap_or(column);
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(44.0, 44.0), Sense::click_and_drag());
                    let active = browser.active_tab() == Some(*id);
                    let fill = if active {
                        palette.active
                    } else if response.hovered() {
                        palette.hover
                    } else {
                        palette.background
                    };
                    ui.painter().rect_filled(rect, 10.0, fill);
                    if active {
                        ui.painter().rect_stroke(
                            rect,
                            10.0,
                            Stroke::new(1.0, palette.border),
                            StrokeKind::Inside,
                        );
                    }
                    paint_avatar(ui, rect.shrink(8.0), &display_host(tab.url()));
                    let response = response.on_hover_text(tab.title());
                    if response.clicked() {
                        actions.push(Action::Command(Command::ActivateTab(*id)));
                    }
                    if response.secondary_clicked() {
                        self.menu = Some(Menu::Tab(
                            *id,
                            response.hover_pos().unwrap_or(rect.left_bottom()),
                        ));
                    }
                    response.dnd_set_drag_payload(*id);
                    self.drop(ui, &response, DropTarget::Pinned(index), palette, actions);
                }
            });
            ui.add_space(4.0);
        }
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), Sense::hover());
        self.drop(
            ui,
            &response,
            DropTarget::Pinned(pinned.len()),
            palette,
            actions,
        );
        if response.dnd_hover_payload::<TabId>().is_some() {
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(2.0, palette.text),
            );
        }
    }

    fn group(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        group: &Group,
        item_index: usize,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::ROW_HEIGHT),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter().rect_filled(rect, 8.0, palette.hover);
        }
        ui.painter().circle_filled(
            egui::pos2(rect.left() + 13.0, rect.center().y),
            4.0,
            theme::group_color(group.color()),
        );
        if self
            .rename
            .as_ref()
            .is_some_and(|(id, _)| *id == group.id())
        {
            if let Some((_, text)) = &mut self.rename {
                let edit = ui.put(
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(26.0, 2.0),
                        egui::vec2(rect.width() - 50.0, 28.0),
                    ),
                    egui::TextEdit::singleline(text),
                );
                edit.request_focus();
                if edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    actions.push(Action::Command(Command::RenameGroup {
                        group: group.id(),
                        name: text.clone(),
                    }));
                    self.rename = None;
                } else if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    self.rename = None;
                }
            }
        } else {
            ui.painter().text(
                rect.left_center() + egui::vec2(26.0, 0.0),
                egui::Align2::LEFT_CENTER,
                group.name(),
                FontId::proportional(13.0),
                if group.collapsed() {
                    palette.secondary
                } else {
                    palette.text
                },
            );
            let progress = ui.ctx().animate_bool_with_time(
                egui::Id::unique(("group", group.id().get())),
                !group.collapsed(),
                0.12,
            );
            paint_chevron(
                ui,
                rect.right_center() - egui::vec2(14.0, 0.0),
                progress,
                palette.secondary,
            );
        }
        if response.clicked() && self.rename.is_none() {
            actions.push(Action::Command(Command::SetGroupCollapsed {
                group: group.id(),
                collapsed: !group.collapsed(),
            }));
        }
        if response.secondary_clicked() {
            self.menu = Some(Menu::Group(
                group.id(),
                response.hover_pos().unwrap_or(rect.left_bottom()),
            ));
        }
        self.drop(
            ui,
            &response,
            DropTarget::Group(group.id(), 0),
            palette,
            actions,
        );
        if !group.collapsed() {
            for (index, id) in group.tabs().iter().enumerate() {
                let line = ui.cursor().min.x + 9.0;
                self.tab_row(
                    ui,
                    browser,
                    *id,
                    DropTarget::Group(group.id(), index),
                    12.0,
                    palette,
                    actions,
                );
                let row = ui.min_rect();
                ui.painter().line_segment(
                    [
                        egui::pos2(line, row.bottom() - theme::ROW_HEIGHT),
                        egui::pos2(line, row.bottom()),
                    ],
                    Stroke::new(2.0, theme::group_color(group.color())),
                );
            }
        }
        let _ = item_index;
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_row(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        id: TabId,
        target: DropTarget,
        indent: f32,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let Some(tab) = browser.tab(id) else {
            return;
        };
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::ROW_HEIGHT),
            Sense::click_and_drag(),
        );
        let active = browser.active_tab() == Some(id);
        if active || response.hovered() {
            ui.painter().rect_filled(
                rect,
                8.0,
                if active {
                    palette.active
                } else {
                    palette.hover
                },
            );
        }
        if active {
            ui.painter().rect_stroke(
                rect,
                8.0,
                Stroke::new(1.0, palette.border),
                StrokeKind::Inside,
            );
        }
        let host = display_host(tab.url());
        paint_avatar(
            ui,
            egui::Rect::from_min_size(
                rect.min + egui::vec2(8.0 + indent, 8.0),
                egui::vec2(16.0, 16.0),
            ),
            &host,
        );
        let title = if tab.title().is_empty() {
            &host
        } else {
            tab.title()
        };
        let max_chars = if active || response.hovered() { 22 } else { 26 };
        let label = truncate(title, max_chars);
        ui.painter().text(
            rect.left_center() + egui::vec2(32.0 + indent, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            palette.text,
        );
        if response.clicked() {
            actions.push(Action::Command(Command::ActivateTab(id)));
        }
        if response.middle_clicked() {
            actions.push(Action::Command(Command::CloseTab(id)));
        }
        if response.secondary_clicked() {
            self.menu = Some(Menu::Tab(
                id,
                response.hover_pos().unwrap_or(rect.left_bottom()),
            ));
        }
        if active || response.hovered() {
            let close_rect = egui::Rect::from_center_size(
                rect.right_center() - egui::vec2(15.0, 0.0),
                egui::vec2(20.0, 20.0),
            );
            if ui
                .put(close_rect, egui::Button::new("×").frame(false))
                .clicked()
            {
                actions.push(Action::Command(Command::CloseTab(id)));
            }
        }
        response.dnd_set_drag_payload(id);
        self.drop(ui, &response, target, palette, actions);
    }

    fn drop(
        &self,
        ui: &egui::Ui,
        response: &egui::Response,
        target: DropTarget,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let lower = response
            .hover_pos()
            .is_some_and(|pos| pos.y > response.rect.center().y);
        if response.dnd_hover_payload::<TabId>().is_some() {
            let y = if lower {
                response.rect.bottom()
            } else {
                response.rect.top()
            };
            ui.painter().line_segment(
                [
                    egui::pos2(response.rect.left(), y),
                    egui::pos2(response.rect.right(), y),
                ],
                Stroke::new(2.0, palette.text),
            );
        }
        if let Some(tab) = response.dnd_release_payload::<TabId>() {
            actions.push(Action::Command(Command::MoveTab {
                tab: *tab,
                to: drop_location(target, lower),
            }));
        }
    }

    fn menu(
        &mut self,
        ui: &mut egui::Ui,
        browser: &Browser,
        palette: Palette,
        actions: &mut Vec<Action>,
    ) {
        let Some(menu) = self.menu else {
            return;
        };
        let pos = match menu {
            Menu::Tab(_, pos) | Menu::Group(_, pos) => pos,
        };
        let sidebar = ui.max_rect();
        let x = pos.x.min(sidebar.right() - 180.0).max(sidebar.left() + 4.0);
        let y = pos.y.min(sidebar.bottom() - 210.0).max(sidebar.top() + 4.0);
        let mut close = false;
        egui::Area::new(egui::Id::unique("sidebar_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x, y))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(palette.background)
                    .show(ui, |ui| {
                        ui.set_min_width(170.0);
                        match menu {
                            Menu::Tab(id, _) => {
                                let pinned = browser
                                    .workspace(browser.active_workspace())
                                    .is_some_and(|ws| ws.pinned().contains(&id));
                                if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                                    actions.push(Action::Command(if pinned {
                                        Command::UnpinTab(id)
                                    } else {
                                        Command::PinTab(id)
                                    }));
                                    close = true;
                                }
                                if !pinned && ui.button("Add to new group").clicked() {
                                    actions.push(Action::Command(Command::CreateGroup {
                                        tabs: vec![id],
                                        name: "New Group".to_owned(),
                                        color: GroupColor::Blue,
                                    }));
                                    close = true;
                                }
                                if find_group(browser, id).is_some()
                                    && ui.button("Remove from group").clicked()
                                {
                                    actions.push(Action::Command(Command::MoveTab {
                                        tab: id,
                                        to: Location::Items(usize::MAX),
                                    }));
                                    close = true;
                                }
                                if ui.button("Close").clicked() {
                                    actions.push(Action::Command(Command::CloseTab(id)));
                                    close = true;
                                }
                            }
                            Menu::Group(id, _) => {
                                if ui.button("Rename").clicked() {
                                    let name = find_group_name(browser, id).unwrap_or_default();
                                    self.rename = Some((id, name));
                                    close = true;
                                }
                                ui.label(
                                    egui::RichText::new("Color")
                                        .size(11.0)
                                        .color(palette.secondary),
                                );
                                ui.horizontal(|ui| {
                                    for color in theme::GROUP_COLORS {
                                        let (rect, response) = ui.allocate_exact_size(
                                            egui::vec2(16.0, 16.0),
                                            Sense::click(),
                                        );
                                        ui.painter().circle_filled(
                                            rect.center(),
                                            6.0,
                                            theme::group_color(color),
                                        );
                                        if response.clicked() {
                                            actions.push(Action::Command(Command::SetGroupColor {
                                                group: id,
                                                color,
                                            }));
                                            close = true;
                                        }
                                    }
                                });
                                if ui.button("Ungroup").clicked() {
                                    actions.push(Action::Command(Command::Ungroup(id)));
                                    close = true;
                                }
                                if ui.button("Close group").clicked() {
                                    actions.push(Action::Command(Command::CloseGroup(id)));
                                    close = true;
                                }
                            }
                        }
                    });
            });
        if close || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.menu = None;
        }
    }
}

fn find_group(browser: &Browser, tab: TabId) -> Option<GroupId> {
    let workspace = browser.workspace(browser.active_workspace())?;
    for item in workspace.items() {
        if let Item::Group(group) = item
            && group.tabs().contains(&tab)
        {
            return Some(group.id());
        }
    }
    None
}

fn find_group_name(browser: &Browser, id: GroupId) -> Option<String> {
    let workspace = browser.workspace(browser.active_workspace())?;
    for item in workspace.items() {
        if let Item::Group(group) = item
            && group.id() == id
        {
            return Some(group.name().to_owned());
        }
    }
    None
}

fn display_host(raw: &str) -> String {
    aegis_ipc::page_url(raw).map_or_else(
        |_| "New Tab".to_owned(),
        |url| aegis_ipc::display_host(&url),
    )
}

fn paint_avatar(ui: &egui::Ui, rect: egui::Rect, host: &str) {
    let (letter, color) = avatar(host);
    ui.painter().rect_filled(rect, 4.0, AVATAR_COLORS[color]);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        FontId::proportional(11.0),
        Color32::WHITE,
    );
}

fn paint_chevron(ui: &egui::Ui, center: egui::Pos2, progress: f32, color: Color32) {
    let angle = progress * std::f32::consts::FRAC_PI_2;
    let (sin, cos) = angle.sin_cos();
    let rotate = |x: f32, y: f32| center + egui::vec2(x * cos - y * sin, x * sin + y * cos);
    let tip = rotate(2.0, 0.0);
    let stroke = Stroke::new(1.5, color);
    ui.painter().line_segment([rotate(-2.0, -4.0), tip], stroke);
    ui.painter().line_segment([tip, rotate(-2.0, 4.0)], stroke);
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut result: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_drop_halves() {
        let id = GroupId::from_raw(7);
        assert_eq!(
            drop_location(DropTarget::Item(3), false),
            Location::Items(3)
        );
        assert_eq!(drop_location(DropTarget::Item(3), true), Location::Items(4));
        assert_eq!(
            drop_location(DropTarget::Group(id, 2), true),
            Location::Group(id, 3)
        );
        assert_eq!(
            drop_location(DropTarget::Pinned(0), false),
            Location::Pinned(0)
        );
    }

    #[test]
    fn avatar_is_deterministic() {
        assert_eq!(avatar("example.com"), avatar("example.com"));
        assert_eq!(avatar("example.com").0, 'E');
        assert!(avatar("example.com").1 < 8);
    }
}
