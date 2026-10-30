use std::collections::BTreeSet;
use std::fmt;

use crate::ids::{GroupId, TabId, WorkspaceId};
use crate::model::{Browser, Group, GroupColor, Item, LoadState, Tab, Workspace};
use crate::settings::{Layout, Settings};

/// placement for a newly opened tab
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// append to root items
    End,
    /// insert after the active tab in its list
    AfterActive,
    /// append inside a group
    InGroup(GroupId),
}

/// destination for a moved tab; indices are measured after removal
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    /// pinned position
    Pinned(usize),
    /// root item position
    Items(usize),
    /// position within a group
    Group(GroupId, usize),
}

/// a requested browser state change
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// open in the active workspace
    OpenTab {
        url: String,
        placement: Placement,
        activate: bool,
    },
    /// close a tab
    CloseTab(TabId),
    /// select a tab and its workspace
    ActivateTab(TabId),
    /// request user navigation
    Navigate { tab: TabId, url: String },
    /// record an engine URL report
    SetUrl { tab: TabId, url: String },
    /// record an engine title report
    SetTitle { tab: TabId, title: String },
    /// pin a tab
    PinTab(TabId),
    /// move a pinned tab to the top of root items
    UnpinTab(TabId),
    /// move a tab within its workspace
    MoveTab { tab: TabId, to: Location },
    /// group tabs in the given order
    CreateGroup {
        tabs: Vec<TabId>,
        name: String,
        color: GroupColor,
    },
    /// rename a group
    RenameGroup { group: GroupId, name: String },
    /// recolor a group
    SetGroupColor { group: GroupId, color: GroupColor },
    /// change a group's display state
    SetGroupCollapsed { group: GroupId, collapsed: bool },
    /// move group tabs to root items
    Ungroup(GroupId),
    /// close every tab in a group
    CloseGroup(GroupId),
    /// add a workspace
    AddWorkspace { name: String, color: GroupColor },
    /// rename a workspace
    RenameWorkspace {
        workspace: WorkspaceId,
        name: String,
    },
    /// select a workspace
    SwitchWorkspace(WorkspaceId),
    /// remove a workspace and its tabs
    RemoveWorkspace(WorkspaceId),
    /// replace preferences after normalizing timeouts
    SetSettings(Settings),
    /// change only the view layout
    SetLayout(Layout),
    /// change only compact mode
    SetCompact(bool),
    /// record engine lifecycle without saving it
    SetLoadState { tab: TabId, state: LoadState },
}

/// work for the app or engine after a command
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// create a view at its current URL
    CreateView { tab: TabId, url: String },
    /// destroy a live or suspended view
    DestroyView(TabId),
    /// navigate an existing view
    NavigateView { tab: TabId, url: String },
    /// show a live view
    ShowView(TabId),
    /// hide a live view
    HideView(TabId),
    /// resume a suspended view
    ResumeView(TabId),
    /// exposes a newly assigned group id
    GroupCreated(GroupId),
    /// exposes a newly assigned workspace id
    WorkspaceCreated(WorkspaceId),
    /// save the changed session once
    PersistSession,
}

/// invalid command input
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandError {
    /// tab id is absent
    UnknownTab,
    /// group id is absent
    UnknownGroup,
    /// workspace id is absent
    UnknownWorkspace,
    /// the last workspace cannot be removed
    LastWorkspace,
    /// URL exceeds 8192 bytes
    UrlTooLong,
    /// selected tabs belong to different workspaces
    MixedWorkspaces,
    /// a pinned tab cannot join a group
    PinnedTabInGroup,
    /// a group needs at least one tab
    EmptyGroup,
    /// a tab was selected more than once
    DuplicateTab,
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CommandError {}

impl Browser {
    /// applies a command atomically and returns ordered engine effects
    pub fn apply(&mut self, command: Command, now_ms: u64) -> Result<Vec<Effect>, CommandError> {
        if matches!(
            &command,
            Command::OpenTab { .. }
                | Command::CloseTab(_)
                | Command::PinTab(_)
                | Command::UnpinTab(_)
                | Command::MoveTab { .. }
                | Command::CreateGroup { .. }
                | Command::Ungroup(_)
                | Command::CloseGroup(_)
                | Command::AddWorkspace { .. }
                | Command::RemoveWorkspace(_)
        ) {
            return self.transaction(|next, effects| next.apply_inner(command, now_ms, effects));
        }

        let persist = match &command {
            Command::SetLoadState { .. } => false,
            Command::SetUrl { tab, url } => self.tabs.get(tab).is_some_and(|old| old.url != *url),
            Command::SetTitle { tab, title } => self
                .tabs
                .get(tab)
                .is_some_and(|old| old.title != cap_chars(title, 512)),
            _ => true,
        };
        let mut effects = Vec::new();
        self.apply_inner(command, now_ms, &mut effects)?;
        if persist {
            effects.push(Effect::PersistSession);
        }
        Ok(effects)
    }

    fn transaction(
        &mut self,
        change: impl FnOnce(&mut Browser, &mut Vec<Effect>) -> Result<(), CommandError>,
    ) -> Result<Vec<Effect>, CommandError> {
        let mut next = self.clone();
        let mut effects = Vec::new();
        change(&mut next, &mut effects)?;
        effects.push(Effect::PersistSession);
        *self = next;
        Ok(effects)
    }

    fn apply_inner(
        &mut self,
        command: Command,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        match command {
            Command::OpenTab {
                url,
                placement,
                activate,
            } => self.open_tab(url, placement, activate, now_ms, effects),
            Command::CloseTab(id) => self.close_tab(id, now_ms, effects),
            Command::ActivateTab(id) => self.activate_tab(id, now_ms, effects),
            Command::Navigate { tab, url } => self.navigate(tab, url, effects),
            Command::SetUrl { tab, url } => self.set_url(tab, url),
            Command::SetTitle { tab, title } => self.set_title(tab, title),
            Command::PinTab(id) => self.pin_tab(id),
            Command::UnpinTab(id) => self.unpin_tab(id),
            Command::MoveTab { tab, to } => self.move_tab(tab, to),
            Command::CreateGroup { tabs, name, color } => {
                self.create_group(tabs, name, color, effects)
            }
            Command::RenameGroup { group, name } => self.rename_group(group, name),
            Command::SetGroupColor { group, color } => self.set_group_color(group, color),
            Command::SetGroupCollapsed { group, collapsed } => {
                self.set_group_collapsed(group, collapsed)
            }
            Command::Ungroup(group) => self.ungroup(group),
            Command::CloseGroup(group) => self.close_group(group, now_ms, effects),
            Command::AddWorkspace { name, color } => {
                self.add_workspace(name, color, effects);
                Ok(())
            }
            Command::RenameWorkspace { workspace, name } => self.rename_workspace(workspace, name),
            Command::SwitchWorkspace(id) => self.switch_workspace(id, effects),
            Command::RemoveWorkspace(id) => self.remove_workspace(id, effects),
            Command::SetSettings(settings) => {
                self.settings = settings.normalized();
                Ok(())
            }
            Command::SetLayout(layout) => {
                self.settings.layout = layout;
                Ok(())
            }
            Command::SetCompact(compact) => {
                self.settings.compact = compact;
                Ok(())
            }
            Command::SetLoadState { tab, state } => {
                self.tabs
                    .get_mut(&tab)
                    .ok_or(CommandError::UnknownTab)?
                    .load = state;
                Ok(())
            }
        }
    }

    fn open_tab(
        &mut self,
        url: String,
        placement: Placement,
        activate: bool,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        check_url(&url)?;
        let workspace_index = self
            .workspace_index(self.active_workspace)
            .ok_or(CommandError::UnknownWorkspace)?;
        if let Placement::InGroup(group) = placement
            && self.group_location(group).map(|(index, _)| index) != Some(workspace_index)
        {
            return Err(CommandError::UnknownGroup);
        }
        let id = TabId(self.next_id());
        self.tabs.insert(
            id,
            Tab {
                id,
                url: url.clone(),
                title: String::new(),
                last_active_ms: 0,
                history: vec![url],
                load: LoadState::Unloaded,
            },
        );
        let workspace = &mut self.workspaces[workspace_index];
        match placement {
            Placement::End => workspace.items.push(Item::Tab(id)),
            Placement::InGroup(group) => {
                if let Some(group) = group_mut(workspace, group) {
                    group.tabs.push(id);
                }
            }
            Placement::AfterActive => insert_after_active(workspace, id),
        }
        if activate {
            self.activate_tab(id, now_ms, effects)?;
        }
        Ok(())
    }

    fn close_tab(
        &mut self,
        id: TabId,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        let workspace_index = self
            .tab_workspace_index(id)
            .ok_or(CommandError::UnknownTab)?;
        let order = self.visual_order(self.workspaces[workspace_index].id);
        let active = self.workspaces[workspace_index].active_tab == Some(id);
        let fallback = if active {
            next_survivor(&order, id, &[id])
        } else {
            None
        };
        remove_placement(&mut self.workspaces[workspace_index], id, false);
        let tab = self.tabs.remove(&id).ok_or(CommandError::UnknownTab)?;
        if matches!(tab.load, LoadState::Live | LoadState::Suspended) {
            effects.push(Effect::DestroyView(id));
        }
        if active {
            self.workspaces[workspace_index].active_tab = None;
            if let Some(next) = fallback {
                if self.active_workspace == self.workspaces[workspace_index].id {
                    self.activate_in_workspace(workspace_index, next, now_ms, effects);
                } else {
                    self.workspaces[workspace_index].active_tab = Some(next);
                }
            }
        }
        Ok(())
    }

    fn activate_tab(
        &mut self,
        id: TabId,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        let workspace_index = self
            .tab_workspace_index(id)
            .ok_or(CommandError::UnknownTab)?;
        self.hide_active(effects);
        self.active_workspace = self.workspaces[workspace_index].id;
        self.activate_in_workspace(workspace_index, id, now_ms, effects);
        Ok(())
    }

    fn activate_in_workspace(
        &mut self,
        workspace_index: usize,
        id: TabId,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) {
        self.workspaces[workspace_index].active_tab = Some(id);
        if let Some(tab) = self.tabs.get_mut(&id) {
            tab.last_active_ms = now_ms;
            match tab.load {
                LoadState::Unloaded | LoadState::Discarded => effects.push(Effect::CreateView {
                    tab: id,
                    url: tab.url.clone(),
                }),
                LoadState::Suspended => effects.push(Effect::ResumeView(id)),
                LoadState::Live => effects.push(Effect::ShowView(id)),
            }
        }
    }

    fn hide_active(&self, effects: &mut Vec<Effect>) {
        if let Some(id) = self.active_tab()
            && self
                .tabs
                .get(&id)
                .is_some_and(|tab| tab.load == LoadState::Live)
        {
            effects.push(Effect::HideView(id));
        }
    }

    fn navigate(
        &mut self,
        id: TabId,
        url: String,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        check_url(&url)?;
        let tab = self.tabs.get_mut(&id).ok_or(CommandError::UnknownTab)?;
        if tab.url != url {
            tab.url = url.clone();
            push_history(tab, &url);
            if matches!(tab.load, LoadState::Live | LoadState::Suspended) {
                effects.push(Effect::NavigateView { tab: id, url });
            }
        }
        Ok(())
    }

    fn set_url(&mut self, id: TabId, url: String) -> Result<(), CommandError> {
        check_url(&url)?;
        let tab = self.tabs.get_mut(&id).ok_or(CommandError::UnknownTab)?;
        if tab.url != url {
            tab.url = url.clone();
            push_history(tab, &url);
        }
        Ok(())
    }

    fn set_title(&mut self, id: TabId, title: String) -> Result<(), CommandError> {
        self.tabs
            .get_mut(&id)
            .ok_or(CommandError::UnknownTab)?
            .title = cap_chars(&title, 512);
        Ok(())
    }

    fn pin_tab(&mut self, id: TabId) -> Result<(), CommandError> {
        let index = self
            .tab_workspace_index(id)
            .ok_or(CommandError::UnknownTab)?;
        let workspace = &mut self.workspaces[index];
        if workspace.pinned.contains(&id) {
            return Ok(());
        }
        remove_placement(workspace, id, false);
        workspace.pinned.push(id);
        Ok(())
    }

    fn unpin_tab(&mut self, id: TabId) -> Result<(), CommandError> {
        let index = self
            .tab_workspace_index(id)
            .ok_or(CommandError::UnknownTab)?;
        let workspace = &mut self.workspaces[index];
        if let Some(position) = workspace.pinned.iter().position(|tab| *tab == id) {
            workspace.pinned.remove(position);
            workspace.items.insert(0, Item::Tab(id));
        }
        Ok(())
    }

    fn move_tab(&mut self, id: TabId, to: Location) -> Result<(), CommandError> {
        let workspace_index = self
            .tab_workspace_index(id)
            .ok_or(CommandError::UnknownTab)?;
        if let Location::Group(group, _) = to
            && self.group_location(group).map(|(index, _)| index) != Some(workspace_index)
        {
            return Err(CommandError::UnknownGroup);
        }
        let workspace = &mut self.workspaces[workspace_index];
        let same_group = match to {
            Location::Group(target, _) => workspace.items.iter().any(|item| {
                matches!(item, Item::Group(group) if group.id == target && group.tabs.contains(&id))
            }),
            _ => false,
        };
        remove_placement(workspace, id, same_group);
        match to {
            Location::Pinned(index) => workspace
                .pinned
                .insert(index.min(workspace.pinned.len()), id),
            Location::Items(index) => workspace
                .items
                .insert(index.min(workspace.items.len()), Item::Tab(id)),
            Location::Group(group, index) => {
                if let Some(group) = group_mut(workspace, group) {
                    group.tabs.insert(index.min(group.tabs.len()), id);
                }
            }
        }
        workspace
            .items
            .retain(|item| !matches!(item, Item::Group(group) if group.tabs.is_empty()));
        Ok(())
    }

    fn create_group(
        &mut self,
        tabs: Vec<TabId>,
        name: String,
        color: GroupColor,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        if tabs.is_empty() {
            return Err(CommandError::EmptyGroup);
        }
        let mut seen = BTreeSet::new();
        let mut workspace_index = None;
        for id in &tabs {
            if !seen.insert(*id) {
                return Err(CommandError::DuplicateTab);
            }
            let index = self
                .tab_workspace_index(*id)
                .ok_or(CommandError::UnknownTab)?;
            if workspace_index.is_some_and(|first| first != index) {
                return Err(CommandError::MixedWorkspaces);
            }
            if self.workspaces[index].pinned.contains(id) {
                return Err(CommandError::PinnedTabInGroup);
            }
            workspace_index = Some(index);
        }
        let index = workspace_index.ok_or(CommandError::EmptyGroup)?;
        let workspace = &self.workspaces[index];
        let first_position = workspace
            .items
            .iter()
            .position(|item| match item {
                Item::Tab(id) => *id == tabs[0],
                Item::Group(group) => group.tabs.contains(&tabs[0]),
            })
            .ok_or(CommandError::UnknownTab)?;
        let mut insert_position = 0;
        for item in &workspace.items[..first_position] {
            match item {
                Item::Tab(id) if !seen.contains(id) => insert_position += 1,
                Item::Group(group) if group.tabs.iter().any(|id| !seen.contains(id)) => {
                    insert_position += 1
                }
                _ => {}
            }
        }
        let group_id = GroupId(self.next_id());
        let workspace = &mut self.workspaces[index];
        for id in &tabs {
            remove_placement(workspace, *id, false);
        }
        let position = insert_position.min(workspace.items.len());
        workspace.items.insert(
            position,
            Item::Group(Group {
                id: group_id,
                name: cap_name(&name),
                color,
                collapsed: false,
                tabs,
            }),
        );
        effects.push(Effect::GroupCreated(group_id));
        Ok(())
    }

    fn rename_group(&mut self, id: GroupId, name: String) -> Result<(), CommandError> {
        self.find_group_mut(id)?.name = cap_name(&name);
        Ok(())
    }

    fn set_group_color(&mut self, id: GroupId, color: GroupColor) -> Result<(), CommandError> {
        self.find_group_mut(id)?.color = color;
        Ok(())
    }

    fn set_group_collapsed(&mut self, id: GroupId, collapsed: bool) -> Result<(), CommandError> {
        self.find_group_mut(id)?.collapsed = collapsed;
        Ok(())
    }

    fn ungroup(&mut self, id: GroupId) -> Result<(), CommandError> {
        let (workspace_index, item_index) =
            self.group_location(id).ok_or(CommandError::UnknownGroup)?;
        let workspace = &mut self.workspaces[workspace_index];
        let Item::Group(group) = workspace.items.remove(item_index) else {
            return Err(CommandError::UnknownGroup);
        };
        for (offset, tab) in group.tabs.into_iter().enumerate() {
            workspace.items.insert(item_index + offset, Item::Tab(tab));
        }
        Ok(())
    }

    fn close_group(
        &mut self,
        id: GroupId,
        now_ms: u64,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        let (workspace_index, item_index) =
            self.group_location(id).ok_or(CommandError::UnknownGroup)?;
        let order = self.visual_order(self.workspaces[workspace_index].id);
        let Item::Group(group) = self.workspaces[workspace_index].items.remove(item_index) else {
            return Err(CommandError::UnknownGroup);
        };
        let active_tab = self.workspaces[workspace_index]
            .active_tab
            .filter(|tab| group.tabs.contains(tab));
        let fallback = active_tab.and_then(|tab| next_survivor(&order, tab, &group.tabs));
        for id in group.tabs {
            if let Some(tab) = self.tabs.remove(&id)
                && matches!(tab.load, LoadState::Live | LoadState::Suspended)
            {
                effects.push(Effect::DestroyView(id));
            }
        }
        if active_tab.is_some() {
            self.workspaces[workspace_index].active_tab = None;
            if let Some(next) = fallback {
                if self.active_workspace == self.workspaces[workspace_index].id {
                    self.activate_in_workspace(workspace_index, next, now_ms, effects);
                } else {
                    self.workspaces[workspace_index].active_tab = Some(next);
                }
            }
        }
        Ok(())
    }

    fn add_workspace(&mut self, name: String, color: GroupColor, effects: &mut Vec<Effect>) {
        let id = WorkspaceId(self.next_id());
        self.workspaces.push(Workspace {
            id,
            name: cap_name(&name),
            color,
            pinned: Vec::new(),
            items: Vec::new(),
            active_tab: None,
        });
        effects.push(Effect::WorkspaceCreated(id));
    }

    fn rename_workspace(&mut self, id: WorkspaceId, name: String) -> Result<(), CommandError> {
        let index = self
            .workspace_index(id)
            .ok_or(CommandError::UnknownWorkspace)?;
        self.workspaces[index].name = cap_name(&name);
        Ok(())
    }

    fn switch_workspace(
        &mut self,
        id: WorkspaceId,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        let index = self
            .workspace_index(id)
            .ok_or(CommandError::UnknownWorkspace)?;
        if id == self.active_workspace {
            return Ok(());
        }
        self.hide_active(effects);
        self.active_workspace = id;
        if let Some(tab) = self.workspaces[index].active_tab {
            self.show_tab(tab, effects);
        }
        Ok(())
    }

    fn remove_workspace(
        &mut self,
        id: WorkspaceId,
        effects: &mut Vec<Effect>,
    ) -> Result<(), CommandError> {
        let index = self
            .workspace_index(id)
            .ok_or(CommandError::UnknownWorkspace)?;
        if self.workspaces.len() == 1 {
            return Err(CommandError::LastWorkspace);
        }
        let was_active = id == self.active_workspace;
        let tabs = self.visual_order(id);
        for tab_id in tabs {
            if let Some(tab) = self.tabs.remove(&tab_id)
                && matches!(tab.load, LoadState::Live | LoadState::Suspended)
            {
                effects.push(Effect::DestroyView(tab_id));
            }
        }
        self.workspaces.remove(index);
        if was_active {
            self.active_workspace = self.workspaces[0].id;
            if let Some(tab) = self.workspaces[0].active_tab {
                self.show_tab(tab, effects);
            }
        }
        Ok(())
    }

    fn show_tab(&self, id: TabId, effects: &mut Vec<Effect>) {
        if let Some(tab) = self.tabs.get(&id) {
            match tab.load {
                LoadState::Unloaded | LoadState::Discarded => effects.push(Effect::CreateView {
                    tab: id,
                    url: tab.url.clone(),
                }),
                LoadState::Suspended => effects.push(Effect::ResumeView(id)),
                LoadState::Live => effects.push(Effect::ShowView(id)),
            }
        }
    }

    fn find_group_mut(&mut self, id: GroupId) -> Result<&mut Group, CommandError> {
        let (workspace_index, _) = self.group_location(id).ok_or(CommandError::UnknownGroup)?;
        group_mut(&mut self.workspaces[workspace_index], id).ok_or(CommandError::UnknownGroup)
    }
}

fn check_url(url: &str) -> Result<(), CommandError> {
    if url.len() > 8192 {
        Err(CommandError::UrlTooLong)
    } else {
        Ok(())
    }
}

fn cap_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
fn cap_name(value: &str) -> String {
    cap_chars(value.trim(), 64)
}

fn push_history(tab: &mut Tab, url: &str) {
    tab.history.push(url.to_owned());
    if tab.history.len() > 50 {
        tab.history.remove(0);
    }
}

fn group_mut(workspace: &mut Workspace, id: GroupId) -> Option<&mut Group> {
    for item in &mut workspace.items {
        if let Item::Group(group) = item
            && group.id == id
        {
            return Some(group);
        }
    }
    None
}

fn insert_after_active(workspace: &mut Workspace, id: TabId) {
    let Some(active) = workspace.active_tab else {
        workspace.items.push(Item::Tab(id));
        return;
    };
    if workspace.pinned.contains(&active) {
        workspace.items.insert(0, Item::Tab(id));
        return;
    }
    for (index, item) in workspace.items.iter_mut().enumerate() {
        match item {
            Item::Tab(tab) if *tab == active => {
                workspace.items.insert(index + 1, Item::Tab(id));
                return;
            }
            Item::Group(group) => {
                if let Some(position) = group.tabs.iter().position(|tab| *tab == active) {
                    group.tabs.insert(position + 1, id);
                    return;
                }
            }
            _ => {}
        }
    }
    workspace.items.push(Item::Tab(id));
}

fn remove_placement(workspace: &mut Workspace, id: TabId, retain_empty_group: bool) {
    if let Some(index) = workspace.pinned.iter().position(|tab| *tab == id) {
        workspace.pinned.remove(index);
        return;
    }
    for index in 0..workspace.items.len() {
        match &mut workspace.items[index] {
            Item::Tab(tab) if *tab == id => {
                workspace.items.remove(index);
                return;
            }
            Item::Group(group) => {
                if let Some(position) = group.tabs.iter().position(|tab| *tab == id) {
                    group.tabs.remove(position);
                    if group.tabs.is_empty() && !retain_empty_group {
                        workspace.items.remove(index);
                    }
                    return;
                }
            }
            _ => {}
        }
    }
}

fn next_survivor(order: &[TabId], active: TabId, removed: &[TabId]) -> Option<TabId> {
    let active_position = order.iter().position(|id| *id == active)?;
    for id in &order[active_position + 1..] {
        if !removed.contains(id) {
            return Some(*id);
        }
    }
    for id in order[..active_position].iter().rev() {
        if !removed.contains(id) {
            return Some(*id);
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn open(browser: &mut Browser, url: &str, activate: bool) -> TabId {
        browser
            .apply(
                Command::OpenTab {
                    url: url.into(),
                    placement: Placement::End,
                    activate,
                },
                10,
            )
            .unwrap();
        *browser
            .visual_order(browser.active_workspace())
            .last()
            .unwrap()
    }

    fn failed(browser: &mut Browser, command: Command, error: CommandError) {
        let before = browser.clone();
        assert_eq!(browser.apply(command, 99), Err(error));
        assert_eq!(*browser, before);
    }

    #[test]
    fn invalid_commands_leave_browser_unchanged() {
        let mut browser = Browser::new();
        let pinned = open(&mut browser, "pinned", false);
        browser.apply(Command::PinTab(pinned), 1).unwrap();
        let grouped = open(&mut browser, "grouped", false);
        let loose = open(&mut browser, "loose", false);
        let group = group_id(
            &browser
                .apply(
                    Command::CreateGroup {
                        tabs: vec![grouped],
                        name: "group".into(),
                        color: GroupColor::Blue,
                    },
                    2,
                )
                .unwrap(),
        );
        let second_workspace = workspace_id(
            &browser
                .apply(
                    Command::AddWorkspace {
                        name: "second".into(),
                        color: GroupColor::Green,
                    },
                    3,
                )
                .unwrap(),
        );
        browser
            .apply(Command::SwitchWorkspace(second_workspace), 4)
            .unwrap();
        let other = open(&mut browser, "other", false);
        let unknown_tab = TabId(9999);
        let unknown_group = GroupId(9999);
        let unknown_workspace = WorkspaceId(9999);
        let too_long = "x".repeat(8193);
        let cases = vec![
            (
                "open tab",
                Command::OpenTab {
                    url: too_long.clone(),
                    placement: Placement::End,
                    activate: true,
                },
                CommandError::UrlTooLong,
            ),
            (
                "close tab",
                Command::CloseTab(unknown_tab),
                CommandError::UnknownTab,
            ),
            (
                "activate tab",
                Command::ActivateTab(unknown_tab),
                CommandError::UnknownTab,
            ),
            (
                "navigate",
                Command::Navigate {
                    tab: loose,
                    url: too_long.clone(),
                },
                CommandError::UrlTooLong,
            ),
            (
                "set url",
                Command::SetUrl {
                    tab: loose,
                    url: too_long,
                },
                CommandError::UrlTooLong,
            ),
            (
                "set title",
                Command::SetTitle {
                    tab: unknown_tab,
                    title: "title".into(),
                },
                CommandError::UnknownTab,
            ),
            (
                "pin tab",
                Command::PinTab(unknown_tab),
                CommandError::UnknownTab,
            ),
            (
                "unpin tab",
                Command::UnpinTab(unknown_tab),
                CommandError::UnknownTab,
            ),
            (
                "move tab",
                Command::MoveTab {
                    tab: loose,
                    to: Location::Group(unknown_group, 0),
                },
                CommandError::UnknownGroup,
            ),
            (
                "create group pinned",
                Command::CreateGroup {
                    tabs: vec![pinned],
                    name: "bad".into(),
                    color: GroupColor::Red,
                },
                CommandError::PinnedTabInGroup,
            ),
            (
                "create group mixed",
                Command::CreateGroup {
                    tabs: vec![loose, other],
                    name: "bad".into(),
                    color: GroupColor::Red,
                },
                CommandError::MixedWorkspaces,
            ),
            (
                "rename group",
                Command::RenameGroup {
                    group: unknown_group,
                    name: "bad".into(),
                },
                CommandError::UnknownGroup,
            ),
            (
                "set group color",
                Command::SetGroupColor {
                    group: unknown_group,
                    color: GroupColor::Red,
                },
                CommandError::UnknownGroup,
            ),
            (
                "set group collapsed",
                Command::SetGroupCollapsed {
                    group: unknown_group,
                    collapsed: true,
                },
                CommandError::UnknownGroup,
            ),
            (
                "ungroup",
                Command::Ungroup(unknown_group),
                CommandError::UnknownGroup,
            ),
            (
                "close group",
                Command::CloseGroup(unknown_group),
                CommandError::UnknownGroup,
            ),
            (
                "rename workspace",
                Command::RenameWorkspace {
                    workspace: unknown_workspace,
                    name: "bad".into(),
                },
                CommandError::UnknownWorkspace,
            ),
            (
                "switch workspace",
                Command::SwitchWorkspace(unknown_workspace),
                CommandError::UnknownWorkspace,
            ),
            (
                "remove workspace",
                Command::RemoveWorkspace(unknown_workspace),
                CommandError::UnknownWorkspace,
            ),
            (
                "set load state",
                Command::SetLoadState {
                    tab: unknown_tab,
                    state: LoadState::Live,
                },
                CommandError::UnknownTab,
            ),
        ];
        for (name, command, error) in cases {
            let mut candidate = browser.clone();
            let before = candidate.clone();
            assert_eq!(candidate.apply(command, 99), Err(error), "{name}");
            assert_eq!(candidate, before, "{name}");
        }

        assert!(browser.group_location(group).is_some());
        let mut last_workspace = Browser::new();
        let before = last_workspace.clone();
        assert_eq!(
            last_workspace.apply(
                Command::RemoveWorkspace(last_workspace.active_workspace()),
                99
            ),
            Err(CommandError::LastWorkspace)
        );
        assert_eq!(last_workspace, before);
    }

    fn group_id(effects: &[Effect]) -> GroupId {
        effects
            .iter()
            .find_map(|effect| {
                if let Effect::GroupCreated(id) = effect {
                    Some(*id)
                } else {
                    None
                }
            })
            .unwrap()
    }

    fn workspace_id(effects: &[Effect]) -> WorkspaceId {
        effects
            .iter()
            .find_map(|effect| {
                if let Effect::WorkspaceCreated(id) = effect {
                    Some(*id)
                } else {
                    None
                }
            })
            .unwrap()
    }

    #[test]
    fn open_activate_and_load_effects() {
        let mut browser = Browser::new();
        let first = open(&mut browser, "https://a.test", false);
        assert_eq!(browser.tab(first).unwrap().load(), LoadState::Unloaded);
        let effects = browser.apply(Command::ActivateTab(first), 20).unwrap();
        assert_eq!(
            effects,
            vec![
                Effect::CreateView {
                    tab: first,
                    url: "https://a.test".into()
                },
                Effect::PersistSession
            ]
        );
        browser
            .apply(
                Command::SetLoadState {
                    tab: first,
                    state: LoadState::Live,
                },
                21,
            )
            .unwrap();
        let second = open(&mut browser, "https://b.test", false);
        let effects = browser.apply(Command::ActivateTab(second), 22).unwrap();
        assert_eq!(effects[0], Effect::HideView(first));
        assert!(effects.contains(&Effect::CreateView {
            tab: second,
            url: "https://b.test".into()
        }));
        for (state, expected) in [
            (LoadState::Live, Effect::ShowView(second)),
            (LoadState::Suspended, Effect::ResumeView(second)),
            (
                LoadState::Discarded,
                Effect::CreateView {
                    tab: second,
                    url: "https://b.test".into(),
                },
            ),
        ] {
            browser
                .apply(Command::SetLoadState { tab: second, state }, 23)
                .unwrap();
            let effects = browser.apply(Command::ActivateTab(second), 24).unwrap();
            assert!(effects.contains(&expected));
        }
        failed(
            &mut browser,
            Command::OpenTab {
                url: "x".repeat(8193),
                placement: Placement::End,
                activate: true,
            },
            CommandError::UrlTooLong,
        );
        failed(
            &mut browser,
            Command::ActivateTab(TabId(999)),
            CommandError::UnknownTab,
        );
        failed(
            &mut browser,
            Command::SetLoadState {
                tab: TabId(999),
                state: LoadState::Live,
            },
            CommandError::UnknownTab,
        );
    }

    #[test]
    fn close_chooses_next_then_previous() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        let b = open(&mut browser, "b", false);
        let c = open(&mut browser, "c", false);
        browser.apply(Command::ActivateTab(b), 1).unwrap();
        browser.apply(Command::CloseTab(b), 2).unwrap();
        assert_eq!(browser.active_tab(), Some(c));
        browser.apply(Command::CloseTab(c), 3).unwrap();
        assert_eq!(browser.active_tab(), Some(a));
        browser.apply(Command::CloseTab(a), 4).unwrap();
        assert_eq!(browser.active_tab(), None);
        failed(&mut browser, Command::CloseTab(a), CommandError::UnknownTab);
    }

    #[test]
    fn group_and_pinned_close_rules() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        let b = open(&mut browser, "b", false);
        let c = open(&mut browser, "c", false);
        browser.apply(Command::PinTab(a), 1).unwrap();
        browser.apply(Command::ActivateTab(a), 2).unwrap();
        browser.apply(Command::CloseTab(a), 3).unwrap();
        assert_eq!(browser.active_tab(), Some(b));
        let group = group_id(
            &browser
                .apply(
                    Command::CreateGroup {
                        tabs: vec![b, c],
                        name: " G ".into(),
                        color: GroupColor::Blue,
                    },
                    4,
                )
                .unwrap(),
        );
        browser.apply(Command::ActivateTab(b), 5).unwrap();
        browser.apply(Command::CloseTab(b), 6).unwrap();
        assert_eq!(browser.active_tab(), Some(c));
        browser.apply(Command::CloseTab(c), 7).unwrap();
        assert_eq!(browser.active_tab(), None);
        assert!(browser.group_location(group).is_none());
        failed(
            &mut browser,
            Command::CloseGroup(group),
            CommandError::UnknownGroup,
        );
    }

    #[test]
    fn visual_order_includes_collapsed_groups() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        let b = open(&mut browser, "b", false);
        let c = open(&mut browser, "c", false);
        browser.apply(Command::PinTab(c), 1).unwrap();
        let group = group_id(
            &browser
                .apply(
                    Command::CreateGroup {
                        tabs: vec![b],
                        name: "g".into(),
                        color: GroupColor::Red,
                    },
                    2,
                )
                .unwrap(),
        );
        browser
            .apply(
                Command::SetGroupCollapsed {
                    group,
                    collapsed: true,
                },
                3,
            )
            .unwrap();
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![c, a, b]
        );
        failed(
            &mut browser,
            Command::SetGroupCollapsed {
                group: GroupId(999),
                collapsed: true,
            },
            CommandError::UnknownGroup,
        );
    }

    #[test]
    fn navigation_reports_and_history_cap() {
        let mut browser = Browser::new();
        let tab = open(&mut browser, "start", true);
        browser
            .apply(
                Command::SetLoadState {
                    tab,
                    state: LoadState::Live,
                },
                1,
            )
            .unwrap();
        let effects = browser
            .apply(
                Command::Navigate {
                    tab,
                    url: "next".into(),
                },
                2,
            )
            .unwrap();
        assert!(effects.contains(&Effect::NavigateView {
            tab,
            url: "next".into()
        }));
        let effects = browser
            .apply(
                Command::SetUrl {
                    tab,
                    url: "reported".into(),
                },
                3,
            )
            .unwrap();
        assert_eq!(effects, vec![Effect::PersistSession]);
        let effects = browser
            .apply(
                Command::SetTitle {
                    tab,
                    title: "x".repeat(600),
                },
                4,
            )
            .unwrap();
        assert_eq!(effects, vec![Effect::PersistSession]);
        assert_eq!(browser.tab(tab).unwrap().title().len(), 512);
        for index in 0..60 {
            browser
                .apply(
                    Command::SetUrl {
                        tab,
                        url: index.to_string(),
                    },
                    5,
                )
                .unwrap();
        }
        assert_eq!(browser.tab(tab).unwrap().history().len(), 50);
        assert_eq!(browser.tab(tab).unwrap().history().first().unwrap(), "10");
        failed(
            &mut browser,
            Command::Navigate {
                tab,
                url: "x".repeat(8193),
            },
            CommandError::UrlTooLong,
        );
        failed(
            &mut browser,
            Command::SetUrl {
                tab: TabId(999),
                url: "x".into(),
            },
            CommandError::UnknownTab,
        );
        failed(
            &mut browser,
            Command::SetTitle {
                tab: TabId(999),
                title: "x".into(),
            },
            CommandError::UnknownTab,
        );
    }

    #[test]
    fn unchanged_engine_reports_do_not_persist() {
        let mut browser = Browser::new();
        let tab = open(&mut browser, "start", false);
        assert!(
            browser
                .apply(
                    Command::SetUrl {
                        tab,
                        url: "start".into(),
                    },
                    1,
                )
                .unwrap()
                .is_empty()
        );
        assert!(
            browser
                .apply(
                    Command::SetTitle {
                        tab,
                        title: String::new(),
                    },
                    2,
                )
                .unwrap()
                .is_empty()
        );
        browser
            .apply(
                Command::SetTitle {
                    tab,
                    title: "x".repeat(513),
                },
                3,
            )
            .unwrap();
        assert!(
            browser
                .apply(
                    Command::SetTitle {
                        tab,
                        title: "x".repeat(514),
                    },
                    4,
                )
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn pin_unpin_move_and_group_commands() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        let b = open(&mut browser, "b", false);
        let c = open(&mut browser, "c", false);
        let group = group_id(
            &browser
                .apply(
                    Command::CreateGroup {
                        tabs: vec![b, c],
                        name: " g ".into(),
                        color: GroupColor::Grey,
                    },
                    1,
                )
                .unwrap(),
        );
        assert_eq!(browser.workspaces()[0].items().len(), 2);
        browser
            .apply(
                Command::RenameGroup {
                    group,
                    name: " new ".into(),
                },
                2,
            )
            .unwrap();
        browser
            .apply(
                Command::SetGroupColor {
                    group,
                    color: GroupColor::Cyan,
                },
                3,
            )
            .unwrap();
        browser
            .apply(
                Command::SetGroupCollapsed {
                    group,
                    collapsed: true,
                },
                4,
            )
            .unwrap();
        let (_, index) = browser.group_location(group).unwrap();
        let Item::Group(info) = &browser.workspaces()[0].items()[index] else {
            panic!()
        };
        assert_eq!(info.name(), "new");
        assert_eq!(info.color(), GroupColor::Cyan);
        assert!(info.collapsed());
        browser.apply(Command::PinTab(b), 5).unwrap();
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![b, a, c]
        );
        browser.apply(Command::UnpinTab(b), 6).unwrap();
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![b, a, c]
        );
        browser
            .apply(
                Command::MoveTab {
                    tab: a,
                    to: Location::Group(group, 99),
                },
                7,
            )
            .unwrap();
        browser
            .apply(
                Command::MoveTab {
                    tab: a,
                    to: Location::Group(group, 0),
                },
                8,
            )
            .unwrap();
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![b, a, c]
        );
        browser.apply(Command::Ungroup(group), 9).unwrap();
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![b, a, c]
        );
        failed(
            &mut browser,
            Command::PinTab(TabId(999)),
            CommandError::UnknownTab,
        );
        failed(
            &mut browser,
            Command::UnpinTab(TabId(999)),
            CommandError::UnknownTab,
        );
        failed(
            &mut browser,
            Command::MoveTab {
                tab: a,
                to: Location::Group(group, 0),
            },
            CommandError::UnknownGroup,
        );
        failed(
            &mut browser,
            Command::RenameGroup {
                group,
                name: "x".into(),
            },
            CommandError::UnknownGroup,
        );
        failed(
            &mut browser,
            Command::SetGroupColor {
                group,
                color: GroupColor::Red,
            },
            CommandError::UnknownGroup,
        );
        failed(
            &mut browser,
            Command::Ungroup(group),
            CommandError::UnknownGroup,
        );
    }

    #[test]
    fn group_creation_rejects_bad_selection() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        failed(
            &mut browser,
            Command::CreateGroup {
                tabs: vec![],
                name: "g".into(),
                color: GroupColor::Grey,
            },
            CommandError::EmptyGroup,
        );
        failed(
            &mut browser,
            Command::CreateGroup {
                tabs: vec![a, a],
                name: "g".into(),
                color: GroupColor::Grey,
            },
            CommandError::DuplicateTab,
        );
        browser.apply(Command::PinTab(a), 1).unwrap();
        failed(
            &mut browser,
            Command::CreateGroup {
                tabs: vec![a],
                name: "g".into(),
                color: GroupColor::Grey,
            },
            CommandError::PinnedTabInGroup,
        );
        failed(
            &mut browser,
            Command::OpenTab {
                url: "x".into(),
                placement: Placement::InGroup(GroupId(999)),
                activate: false,
            },
            CommandError::UnknownGroup,
        );
    }

    #[test]
    fn workspace_commands_and_layout_preserve_tabs() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", true);
        let first = browser.active_workspace();
        let second = workspace_id(
            &browser
                .apply(
                    Command::AddWorkspace {
                        name: " Work ".into(),
                        color: GroupColor::Pink,
                    },
                    1,
                )
                .unwrap(),
        );
        browser
            .apply(
                Command::RenameWorkspace {
                    workspace: second,
                    name: "Office".into(),
                },
                2,
            )
            .unwrap();
        browser.apply(Command::SwitchWorkspace(second), 3).unwrap();
        let b = open(&mut browser, "b", true);
        assert_eq!(browser.active_tab(), Some(b));
        browser.apply(Command::SwitchWorkspace(first), 4).unwrap();
        assert_eq!(browser.active_tab(), Some(a));
        let saved_tabs = browser.tabs.clone();
        let saved_workspaces = browser.workspaces.clone();
        browser
            .apply(Command::SetLayout(Layout::Horizontal), 5)
            .unwrap();
        browser.apply(Command::SetCompact(true), 6).unwrap();
        assert_eq!(browser.tabs, saved_tabs);
        assert_eq!(browser.workspaces, saved_workspaces);
        browser
            .apply(
                Command::SetSettings(Settings {
                    suspend_after_minutes: 0,
                    discard_after_minutes: 0,
                    ..Settings::default()
                }),
                7,
            )
            .unwrap();
        assert_eq!(browser.settings().suspend_after_minutes, 1);
        assert_eq!(browser.settings().discard_after_minutes, 1);
        browser.apply(Command::RemoveWorkspace(second), 8).unwrap();
        assert!(browser.tab(b).is_none());
        failed(
            &mut browser,
            Command::RemoveWorkspace(first),
            CommandError::LastWorkspace,
        );
        failed(
            &mut browser,
            Command::SwitchWorkspace(second),
            CommandError::UnknownWorkspace,
        );
        failed(
            &mut browser,
            Command::RenameWorkspace {
                workspace: second,
                name: "x".into(),
            },
            CommandError::UnknownWorkspace,
        );
    }

    #[test]
    fn close_group_selects_after_active_tab() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        let b = open(&mut browser, "b", false);
        let c = open(&mut browser, "c", false);
        let d = open(&mut browser, "d", false);
        let group = group_id(
            &browser
                .apply(
                    Command::CreateGroup {
                        tabs: vec![b, c],
                        name: "g".into(),
                        color: GroupColor::Grey,
                    },
                    1,
                )
                .unwrap(),
        );
        browser.apply(Command::ActivateTab(c), 2).unwrap();
        browser.apply(Command::CloseGroup(group), 3).unwrap();
        assert_eq!(browser.active_tab(), Some(d));
        assert_eq!(browser.visual_order(browser.active_workspace()), vec![a, d]);
    }

    #[test]
    fn move_indices_use_the_list_after_removal() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        let b = open(&mut browser, "b", false);
        let c = open(&mut browser, "c", false);
        let group = group_id(
            &browser
                .apply(
                    Command::CreateGroup {
                        tabs: vec![b],
                        name: "g".into(),
                        color: GroupColor::Grey,
                    },
                    1,
                )
                .unwrap(),
        );
        browser
            .apply(
                Command::MoveTab {
                    tab: b,
                    to: Location::Items(1),
                },
                2,
            )
            .unwrap();
        assert!(browser.group_location(group).is_none());
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![a, b, c]
        );
        browser
            .apply(
                Command::MoveTab {
                    tab: a,
                    to: Location::Pinned(99),
                },
                3,
            )
            .unwrap();
        browser
            .apply(
                Command::MoveTab {
                    tab: c,
                    to: Location::Pinned(0),
                },
                4,
            )
            .unwrap();
        assert_eq!(browser.workspaces()[0].pinned(), &[c, a]);
        assert_eq!(browser.check_invariants(), Ok(()));
    }

    #[test]
    fn grouping_uses_first_selected_tabs_original_position() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", false);
        let b = open(&mut browser, "b", false);
        let c = open(&mut browser, "c", false);
        let d = open(&mut browser, "d", false);
        let group = group_id(
            &browser
                .apply(
                    Command::CreateGroup {
                        tabs: vec![c, a],
                        name: "g".into(),
                        color: GroupColor::Blue,
                    },
                    1,
                )
                .unwrap(),
        );
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![b, c, a, d]
        );
        browser.apply(Command::Ungroup(group), 2).unwrap();
        assert_eq!(
            browser.visual_order(browser.active_workspace()),
            vec![b, c, a, d]
        );
    }

    #[test]
    fn inactive_workspace_close_does_not_show_a_view() {
        let mut browser = Browser::new();
        let a = open(&mut browser, "a", true);
        let b = open(&mut browser, "b", false);
        let first = browser.active_workspace();
        let second = workspace_id(
            &browser
                .apply(
                    Command::AddWorkspace {
                        name: "second".into(),
                        color: GroupColor::Grey,
                    },
                    1,
                )
                .unwrap(),
        );
        browser.apply(Command::SwitchWorkspace(second), 2).unwrap();
        let effects = browser.apply(Command::CloseTab(a), 3).unwrap();
        assert!(!effects.iter().any(|effect| matches!(
            effect,
            Effect::CreateView { .. } | Effect::ShowView(_) | Effect::ResumeView(_)
        )));
        assert_eq!(browser.workspace(first).unwrap().active_tab(), Some(b));
    }

    #[test]
    fn lazy_open_and_load_report_have_no_view_or_persist_effect() {
        let mut browser = Browser::new();
        let effects = browser
            .apply(
                Command::OpenTab {
                    url: "x".into(),
                    placement: Placement::End,
                    activate: false,
                },
                1,
            )
            .unwrap();
        assert_eq!(effects, vec![Effect::PersistSession]);
        let tab = browser.visual_order(browser.active_workspace())[0];
        let effects = browser
            .apply(
                Command::SetLoadState {
                    tab,
                    state: LoadState::Live,
                },
                2,
            )
            .unwrap();
        assert!(effects.is_empty());
    }

    #[test]
    fn randomized_commands_preserve_invariants() {
        for seed in 1..=5 {
            let mut random = seed;
            let mut browser = Browser::new();
            for step in 0..2000 {
                let choice = roll(&mut random) % 20;
                let tab = TabId(roll(&mut random) % browser.next_id.saturating_add(5));
                let group = GroupId(roll(&mut random) % browser.next_id.saturating_add(5));
                let workspace = WorkspaceId(roll(&mut random) % browser.next_id.saturating_add(5));
                let command = match choice {
                    0 => Command::OpenTab {
                        url: format!("https://{seed}/{step}"),
                        placement: Placement::End,
                        activate: true,
                    },
                    1 => Command::OpenTab {
                        url: format!("https://{seed}/{step}"),
                        placement: Placement::AfterActive,
                        activate: false,
                    },
                    2 => Command::OpenTab {
                        url: "x".into(),
                        placement: Placement::InGroup(group),
                        activate: false,
                    },
                    3 => Command::CloseTab(tab),
                    4 => Command::ActivateTab(tab),
                    5 => Command::Navigate {
                        tab,
                        url: step.to_string(),
                    },
                    6 => Command::SetUrl {
                        tab,
                        url: format!("report/{step}"),
                    },
                    7 => Command::SetTitle {
                        tab,
                        title: format!("title {step}"),
                    },
                    8 => Command::PinTab(tab),
                    9 => Command::UnpinTab(tab),
                    10 => Command::MoveTab {
                        tab,
                        to: Location::Items((roll(&mut random) % 10) as usize),
                    },
                    11 => Command::MoveTab {
                        tab,
                        to: Location::Group(group, 0),
                    },
                    12 => Command::CreateGroup {
                        tabs: vec![tab],
                        name: "g".into(),
                        color: GroupColor::Blue,
                    },
                    13 => Command::Ungroup(group),
                    14 => Command::CloseGroup(group),
                    15 => Command::AddWorkspace {
                        name: "w".into(),
                        color: GroupColor::Grey,
                    },
                    16 => Command::SwitchWorkspace(workspace),
                    17 => Command::RemoveWorkspace(workspace),
                    18 => Command::SetGroupCollapsed {
                        group,
                        collapsed: step % 2 == 0,
                    },
                    _ => Command::SetLoadState {
                        tab,
                        state: LoadState::Live,
                    },
                };
                let before = browser.clone();
                if browser.apply(command, step).is_err() {
                    assert_eq!(browser, before);
                }
                assert_eq!(
                    browser.check_invariants(),
                    Ok(()),
                    "seed {seed}, step {step}"
                );
            }
        }
    }

    fn roll(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }
}
