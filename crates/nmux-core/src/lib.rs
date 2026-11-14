pub mod detect;
pub mod layout;
pub mod model;
pub mod validate;

pub use layout::{Dir, Node, PaneId, Rect, Side};
pub use model::{
    ssh_failed,
    AgentState, Closed, Created, Detected, Machine, MachineId, MachineStatus, ModelError, Pane, PaneKind, PaneSpec,
    PaneStatus, Settings, ShellClaude, State, Tab, TabId, Workspace, WorkspaceId,
};
