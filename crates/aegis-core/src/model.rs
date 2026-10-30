use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::{GroupId, TabId, WorkspaceId};
use crate::settings::Settings;

/// the browser's saved state and tab lookup
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Browser {
    pub(crate) workspaces: Vec<Workspace>,
    pub(crate) active_workspace: WorkspaceId,
    pub(crate) tabs: BTreeMap<TabId, Tab>,
    pub(crate) settings: Settings,
    pub(crate) next_id: u64,
}

/// one named collection of tabs and groups
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub(crate) id: WorkspaceId,
    pub(crate) name: String,
    pub(crate) color: GroupColor,
    pub(crate) pinned: Vec<TabId>,
    pub(crate) items: Vec<Item>,
    pub(crate) active_tab: Option<TabId>,
}

/// a root tab or an ordered group
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    /// a root tab
    Tab(TabId),
    /// a group of tabs
    Group(Group),
}

/// a nonempty ordered tab group
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub(crate) id: GroupId,
    pub(crate) name: String,
    pub(crate) color: GroupColor,
    pub(crate) collapsed: bool,
    pub(crate) tabs: Vec<TabId>,
}

/// a tab's saved navigation state and runtime load state
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub(crate) id: TabId,
    #[serde(default)]
    pub(crate) opener: Option<TabId>,
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) last_active_ms: u64,
    pub(crate) history: Vec<String>,
    #[serde(skip)]
    pub(crate) load: LoadState,
}

/// runtime state of a tab's engine view
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadState {
    /// no view has been created
    #[default]
    Unloaded,
    /// a live view exists
    Live,
    /// a paused view exists
    Suspended,
    /// the previous view was discarded
    Discarded,
}

/// colors available to groups and workspaces
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupColor {
    /// neutral grey
    #[default]
    Grey,
    /// blue
    Blue,
    /// red
    Red,
    /// yellow
    Yellow,
    /// green
    Green,
    /// pink
    Pink,
    /// purple
    Purple,
    /// cyan
    Cyan,
    /// orange
    Orange,
}

/// a broken relationship in browser state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvariantError {
    /// no workspaces remain
    NoWorkspaces,
    /// a workspace id appears twice
    DuplicateWorkspace,
    /// a group id appears twice
    DuplicateGroup,
    /// a group has no tabs
    EmptyGroup,
    /// a placement refers to an absent tab
    MissingTab,
    /// a tab has multiple placements
    DuplicateTab,
    /// a tab has no placement
    UnplacedTab,
    /// a tab's stored id differs from its lookup id
    MismatchedTabId,
    /// a tab's opener is absent
    MissingOpener,
    /// the active workspace is absent
    MissingActiveWorkspace,
    /// an active tab belongs elsewhere
    InvalidActiveTab,
    /// the id counter could reuse an existing id
    InvalidNextId,
}

impl fmt::Display for InvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for InvariantError {}

impl Default for Browser {
    fn default() -> Self {
        Self::new()
    }
}

impl Browser {
    /// creates an empty Home workspace
    pub fn new() -> Self {
        let id = WorkspaceId(1);
        Self {
            workspaces: vec![Workspace {
                id,
                name: "Home".into(),
                color: GroupColor::Grey,
                pinned: Vec::new(),
                items: Vec::new(),
                active_tab: None,
            }],
            active_workspace: id,
            tabs: BTreeMap::new(),
            settings: Settings::default(),
            next_id: 2,
        }
    }

    /// returns workspaces in display order
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }
    /// finds a workspace by id
    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|workspace| workspace.id == id)
    }
    /// finds a tab by id
    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.get(&id)
    }
    /// returns the selected workspace
    pub fn active_workspace(&self) -> WorkspaceId {
        self.active_workspace
    }
    /// returns the active tab in the selected workspace
    pub fn active_tab(&self) -> Option<TabId> {
        self.workspace(self.active_workspace)
            .and_then(Workspace::active_tab)
    }
    /// returns the current preferences
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// returns every tab in pinned and visual item order
    pub fn visual_order(&self, workspace: WorkspaceId) -> Vec<TabId> {
        let Some(workspace) = self.workspace(workspace) else {
            return Vec::new();
        };
        let mut order = workspace.pinned.clone();
        for item in &workspace.items {
            match item {
                Item::Tab(id) => order.push(*id),
                Item::Group(group) => order.extend(&group.tabs),
            }
        }
        order
    }

    /// verifies ids, placements, and active selections
    pub fn check_invariants(&self) -> Result<(), InvariantError> {
        if self.workspaces.is_empty() {
            return Err(InvariantError::NoWorkspaces);
        }
        let mut workspaces = BTreeSet::new();
        let mut groups = BTreeSet::new();
        let mut placed = BTreeSet::new();
        let mut highest = 0;
        for workspace in &self.workspaces {
            if !workspaces.insert(workspace.id) {
                return Err(InvariantError::DuplicateWorkspace);
            }
            highest = highest.max(workspace.id.0);
            for id in &workspace.pinned {
                self.check_placement(*id, &mut placed)?;
            }
            for item in &workspace.items {
                match item {
                    Item::Tab(id) => self.check_placement(*id, &mut placed)?,
                    Item::Group(group) => {
                        if !groups.insert(group.id) {
                            return Err(InvariantError::DuplicateGroup);
                        }
                        if group.tabs.is_empty() {
                            return Err(InvariantError::EmptyGroup);
                        }
                        highest = highest.max(group.id.0);
                        for id in &group.tabs {
                            self.check_placement(*id, &mut placed)?;
                        }
                    }
                }
            }
            if let Some(id) = workspace.active_tab
                && !self.visual_order(workspace.id).contains(&id)
            {
                return Err(InvariantError::InvalidActiveTab);
            }
        }
        if !workspaces.contains(&self.active_workspace) {
            return Err(InvariantError::MissingActiveWorkspace);
        }
        if placed.len() != self.tabs.len() {
            return Err(InvariantError::UnplacedTab);
        }
        for (id, tab) in &self.tabs {
            if *id != tab.id {
                return Err(InvariantError::MismatchedTabId);
            }
            if let Some(opener) = tab.opener
                && !self.tabs.contains_key(&opener)
            {
                return Err(InvariantError::MissingOpener);
            }
            highest = highest.max(id.0);
        }
        if self.next_id <= highest {
            return Err(InvariantError::InvalidNextId);
        }
        Ok(())
    }

    fn check_placement(
        &self,
        id: TabId,
        placed: &mut BTreeSet<TabId>,
    ) -> Result<(), InvariantError> {
        if !self.tabs.contains_key(&id) {
            return Err(InvariantError::MissingTab);
        }
        if !placed.insert(id) {
            return Err(InvariantError::DuplicateTab);
        }
        Ok(())
    }

    pub(crate) fn workspace_index(&self, id: WorkspaceId) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|workspace| workspace.id == id)
    }

    pub(crate) fn tab_workspace_index(&self, id: TabId) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|workspace| workspace.contains(id))
    }

    pub(crate) fn group_location(&self, id: GroupId) -> Option<(usize, usize)> {
        for (workspace_index, workspace) in self.workspaces.iter().enumerate() {
            for (item_index, item) in workspace.items.iter().enumerate() {
                if matches!(item, Item::Group(group) if group.id == id) {
                    return Some((workspace_index, item_index));
                }
            }
        }
        None
    }

    pub(crate) fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

impl Workspace {
    /// returns the workspace id
    pub fn id(&self) -> WorkspaceId {
        self.id
    }
    /// returns the display name
    pub fn name(&self) -> &str {
        &self.name
    }
    /// returns the workspace color
    pub fn color(&self) -> GroupColor {
        self.color
    }
    /// returns pinned tabs in order
    pub fn pinned(&self) -> &[TabId] {
        &self.pinned
    }
    /// returns root tabs and groups in order
    pub fn items(&self) -> &[Item] {
        &self.items
    }
    /// returns the selected tab in this workspace
    pub fn active_tab(&self) -> Option<TabId> {
        self.active_tab
    }

    pub(crate) fn contains(&self, id: TabId) -> bool {
        if self.pinned.contains(&id) {
            return true;
        }
        for item in &self.items {
            match item {
                Item::Tab(tab) if *tab == id => return true,
                Item::Group(group) if group.tabs.contains(&id) => return true,
                _ => {}
            }
        }
        false
    }
}

impl Group {
    /// returns the group id
    pub fn id(&self) -> GroupId {
        self.id
    }
    /// returns the display name
    pub fn name(&self) -> &str {
        &self.name
    }
    /// returns the group color
    pub fn color(&self) -> GroupColor {
        self.color
    }
    /// reports whether the group is collapsed in the UI
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }
    /// returns group tabs in order
    pub fn tabs(&self) -> &[TabId] {
        &self.tabs
    }
}

impl Tab {
    /// returns the tab id
    pub fn id(&self) -> TabId {
        self.id
    }
    /// returns the tab that opened this tab
    pub fn opener(&self) -> Option<TabId> {
        self.opener
    }
    /// returns the current URL
    pub fn url(&self) -> &str {
        &self.url
    }
    /// returns the current title
    pub fn title(&self) -> &str {
        &self.title
    }
    /// returns the last activation time in milliseconds
    pub fn last_active_ms(&self) -> u64 {
        self.last_active_ms
    }
    /// returns up to 50 visited URLs
    pub fn history(&self) -> &[String] {
        &self.history
    }
    /// returns the engine load state
    pub fn load(&self) -> LoadState {
        self.load
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{Command, Placement};

    #[test]
    fn new_browser_is_valid() {
        let browser = Browser::new();
        assert_eq!(browser.workspaces()[0].name(), "Home");
        assert_eq!(browser.check_invariants(), Ok(()));
    }

    #[test]
    fn missing_opener_breaks_invariant() {
        let mut browser = Browser::new();
        browser
            .apply(
                Command::OpenTab {
                    url: "https://popup.test".into(),
                    placement: Placement::End,
                    activate: true,
                    opener: None,
                },
                1,
            )
            .unwrap();
        let tab = browser.active_tab().unwrap();
        browser.tabs.get_mut(&tab).unwrap().opener = Some(TabId(999));
        assert_eq!(
            browser.check_invariants(),
            Err(InvariantError::MissingOpener)
        );
    }
}
