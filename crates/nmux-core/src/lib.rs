//! The Nmux model: workspaces, tabs, split panes and agent state.
//!
//! Pure logic. The server (`nmux-server`) owns processes and I/O and drives
//! this; the UI only ever sees snapshots of [`model::State`].

pub mod detect;
pub mod layout;
pub mod model;
pub mod validate;

pub use layout::{Dir, Node, PaneId, Rect, Side};
pub use model::{
    AgentState, Closed, Created, Detected, Machine, MachineId, MachineStatus, ModelError, Pane, PaneKind, PaneSpec,
    PaneStatus, State, Tab, TabId, Workspace, WorkspaceId,
};
