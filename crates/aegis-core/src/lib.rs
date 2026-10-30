//! browser data model, commands, sessions, and settings
#![forbid(unsafe_code)]

/// state-changing commands and engine effects
pub mod command;
/// stable ids for tabs, groups, and workspaces
pub mod ids;
/// browser state and invariants
pub mod model;
/// session persistence and recovery
pub mod session;
/// saved browser preferences
pub mod settings;

pub use command::{Command, CommandError, Effect, Location, Placement};
pub use ids::{GroupId, TabId, WorkspaceId};
pub use model::{Browser, Group, GroupColor, InvariantError, Item, LoadState, Tab, Workspace};
pub use settings::{Layout, SearchEngine, Settings};
