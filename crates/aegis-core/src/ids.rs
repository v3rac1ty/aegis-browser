use serde::{Deserialize, Serialize};

/// identifies a tab across workspaces
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct TabId(pub(crate) u64);

/// identifies a group across workspaces
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct GroupId(pub(crate) u64);

/// identifies a workspace
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct WorkspaceId(pub(crate) u64);

impl TabId {
    /// returns the stable numeric id
    pub fn get(self) -> u64 {
        self.0
    }
    /// creates an id for lookup or engine reports
    pub fn from_raw(id: u64) -> Self {
        Self(id)
    }
}

impl GroupId {
    /// returns the stable numeric id
    pub fn get(self) -> u64 {
        self.0
    }
    /// creates an id for lookup
    pub fn from_raw(id: u64) -> Self {
        Self(id)
    }
}

impl WorkspaceId {
    /// returns the stable numeric id
    pub fn get(self) -> u64 {
        self.0
    }
    /// creates an id for lookup
    pub fn from_raw(id: u64) -> Self {
        Self(id)
    }
}
