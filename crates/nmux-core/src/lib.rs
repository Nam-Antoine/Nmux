pub mod detect;
pub mod layout;
pub mod model;
pub mod validate;

pub use layout::{Dir, Node, PaneId, Rect, Side};
pub use model::{
    AgentState, Closed, Created, Detected, Machine, MachineId, MachineStatus, ModelError, Pane, PaneKind, PaneSpec,
    PaneStatus, ShellClaude, State, Tab, TabId, Workspace, WorkspaceId,
};
