//! Workspaces, tabs and panes.
//!
//! A workspace is a project folder. It owns tabs; a tab is a split layout of
//! panes; a pane is one real terminal (Claude, a shell, or a cloud launcher).
//! This module only holds the structure and the rules for changing it. The
//! server owns the processes and calls in here; nothing here does I/O.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::layout::{Dir, Node, PaneId, Side};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TabId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneKind {
    Claude,
    Shell,
    /// Runs `claude --cloud "<task>"`, which creates a cloud session and exits.
    Cloud,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneStatus {
    Running,
    Exited,
}

/// What the agent in a pane is doing, as far as Nmux can tell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    /// Needs input, approval or a decision.
    Blocked,
    /// Mid-turn.
    Working,
    /// Finished, and nobody has looked at it yet.
    Done,
    /// Ready for input and already seen.
    Idle,
    /// Not an agent, or cannot be classified.
    #[default]
    Unknown,
}

impl AgentState {
    /// Higher wins when the states of several panes roll up to a tab or workspace.
    pub fn rank(self) -> u8 {
        match self {
            AgentState::Blocked => 4,
            AgentState::Working => 3,
            AgentState::Done => 2,
            AgentState::Idle => 1,
            AgentState::Unknown => 0,
        }
    }
}

/// What the detector read off the terminal, before "has the user seen it" is applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Detected {
    Blocked,
    Working,
    Idle,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub id: PaneId,
    pub kind: PaneKind,
    pub name: String,
    pub cwd: String,
    pub status: PaneStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub agent: AgentState,
    /// Nmux chooses Claude's session id so the conversation can be resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(default)]
    pub skip_permissions: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_url: Option<String>,
    /// Cloud session this Claude pane was teleported from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teleported_from: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub created_at: u64,
}

/// What a caller supplies to create a pane; everything else is filled in.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSpec {
    pub kind: PaneKind,
    #[serde(default)]
    pub name: Option<String>,
    /// Defaults to the workspace folder.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub skip_permissions: bool,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub cloud_task: Option<String>,
    #[serde(default)]
    pub teleport: Option<String>,
}

impl PaneSpec {
    pub fn new(kind: PaneKind) -> PaneSpec {
        PaneSpec {
            kind,
            name: None,
            cwd: None,
            skip_permissions: false,
            extra_args: Vec::new(),
            cloud_task: None,
            teleport: None,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: TabId,
    /// `None` shows the tab's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub layout: Node,
    pub focused: PaneId,
    /// The focused pane fills the tab; the layout is kept.
    #[serde(default)]
    pub zoomed: bool,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub cwd: String,
    pub tabs: Vec<Tab>,
    pub active_tab: TabId,
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub workspaces: Vec<Workspace>,
    /// On the wire this is a plain list: JSON map keys are strings, which
    /// does not survive every way serde reads a message back.
    #[serde(with = "panes_as_list")]
    pub panes: BTreeMap<PaneId, Pane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_workspace: Option<WorkspaceId>,
    /// One counter for every kind of id, so an id is never reused.
    #[serde(default)]
    pub next_id: u32,
}

mod panes_as_list {
    use super::{BTreeMap, Pane, PaneId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(panes: &BTreeMap<PaneId, Pane>, s: S) -> Result<S::Ok, S::Error> {
        panes.values().collect::<Vec<_>>().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<PaneId, Pane>, D::Error> {
        Ok(Vec::<Pane>::deserialize(d)?.into_iter().map(|p| (p.id, p)).collect())
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("no such workspace")]
    NoWorkspace,
    #[error("no such tab")]
    NoTab,
    #[error("no such pane")]
    NoPane,
    #[error("{0}")]
    Invalid(&'static str),
}

/// Everything that disappeared in one close, so the server can stop the
/// processes and the client can drop its terminals.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Closed {
    pub panes: Vec<PaneId>,
    pub tab: Option<TabId>,
    pub workspace: Option<WorkspaceId>,
}

/// Ids of a freshly created pane and where it landed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    pub workspace: WorkspaceId,
    pub tab: TabId,
    pub pane: PaneId,
}

type Res<T> = Result<T, ModelError>;

impl State {
    fn fresh_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    fn workspace_mut(&mut self, id: WorkspaceId) -> Res<&mut Workspace> {
        self.workspaces.iter_mut().find(|w| w.id == id).ok_or(ModelError::NoWorkspace)
    }

    /// Indexes of the workspace and tab that hold a pane.
    pub fn locate(&self, pane: PaneId) -> Option<(usize, usize)> {
        self.workspaces.iter().enumerate().find_map(|(wi, w)| {
            w.tabs.iter().position(|t| t.layout.contains(pane)).map(|ti| (wi, ti))
        })
    }

    fn locate_tab(&self, tab: TabId) -> Option<(usize, usize)> {
        self.workspaces
            .iter()
            .enumerate()
            .find_map(|(wi, w)| w.tabs.iter().position(|t| t.id == tab).map(|ti| (wi, ti)))
    }

    fn tab_of_mut(&mut self, pane: PaneId) -> Res<&mut Tab> {
        let (wi, ti) = self.locate(pane).ok_or(ModelError::NoPane)?;
        Ok(&mut self.workspaces[wi].tabs[ti])
    }

    fn new_pane(&mut self, spec: PaneSpec, workspace_cwd: &str, now: u64) -> Pane {
        let id = PaneId(self.fresh_id());
        let name = spec.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| match spec.kind {
            PaneKind::Claude => "claude".to_string(),
            PaneKind::Shell => "shell".to_string(),
            PaneKind::Cloud => spec
                .cloud_task
                .as_deref()
                .map(|t| truncate(t.lines().next().unwrap_or("").trim(), 48))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "cloud".to_string()),
        });
        Pane {
            id,
            kind: spec.kind,
            name,
            cwd: spec.cwd.unwrap_or_else(|| workspace_cwd.to_string()),
            status: PaneStatus::Running,
            exit_code: None,
            agent: AgentState::Unknown,
            claude_session_id: None,
            skip_permissions: spec.skip_permissions,
            extra_args: spec.extra_args,
            cloud_task: spec.cloud_task,
            cloud_session_id: None,
            cloud_url: None,
            teleported_from: spec.teleport,
            created_at: now,
        }
    }

    /// New workspace with one tab holding one pane. It becomes the active one.
    pub fn create_workspace(&mut self, name: Option<String>, cwd: String, spec: PaneSpec, now: u64) -> Created {
        let pane = self.new_pane(spec, &cwd, now);
        let pane_id = pane.id;
        self.panes.insert(pane_id, pane);
        let tab = TabId(self.fresh_id());
        let workspace = WorkspaceId(self.fresh_id());
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| folder_name(&cwd));
        self.workspaces.push(Workspace {
            id: workspace,
            name,
            cwd,
            tabs: vec![Tab { id: tab, name: None, layout: Node::leaf(pane_id), focused: pane_id, zoomed: false }],
            active_tab: tab,
        });
        self.active_workspace = Some(workspace);
        Created { workspace, tab, pane: pane_id }
    }

    /// New tab at the end of a workspace, holding one pane. It becomes active.
    pub fn create_tab(&mut self, workspace: WorkspaceId, name: Option<String>, spec: PaneSpec, now: u64) -> Res<Created> {
        let cwd = self.workspace(workspace).ok_or(ModelError::NoWorkspace)?.cwd.clone();
        let pane = self.new_pane(spec, &cwd, now);
        let pane_id = pane.id;
        self.panes.insert(pane_id, pane);
        let tab = TabId(self.fresh_id());
        let ws = self.workspace_mut(workspace)?;
        ws.tabs.push(Tab {
            id: tab,
            name: name.filter(|n| !n.trim().is_empty()),
            layout: Node::leaf(pane_id),
            focused: pane_id,
            zoomed: false,
        });
        ws.active_tab = tab;
        self.active_workspace = Some(workspace);
        Ok(Created { workspace, tab, pane: pane_id })
    }

    /// Split `target`; the new pane takes the right/bottom half and the focus.
    pub fn split_pane(&mut self, target: PaneId, dir: Dir, spec: PaneSpec, now: u64) -> Res<Created> {
        let (wi, ti) = self.locate(target).ok_or(ModelError::NoPane)?;
        let cwd = self.workspaces[wi].cwd.clone();
        let pane = self.new_pane(spec, &cwd, now);
        let pane_id = pane.id;
        self.panes.insert(pane_id, pane);
        let ws = &mut self.workspaces[wi];
        let tab = &mut ws.tabs[ti];
        tab.layout.split(target, dir, pane_id);
        tab.focused = pane_id;
        tab.zoomed = false;
        Ok(Created { workspace: ws.id, tab: tab.id, pane: pane_id })
    }

    /// Close one pane. The last pane takes its tab with it, and the last tab
    /// takes its workspace.
    pub fn close_pane(&mut self, pane: PaneId) -> Res<Closed> {
        let (wi, ti) = self.locate(pane).ok_or(ModelError::NoPane)?;
        self.panes.remove(&pane);
        let mut closed = Closed { panes: vec![pane], ..Closed::default() };
        let tab = &mut self.workspaces[wi].tabs[ti];
        match tab.layout.clone().remove(pane) {
            Some(rest) => {
                if tab.focused == pane {
                    tab.focused = rest.panes()[0];
                }
                tab.layout = rest;
                tab.zoomed = false;
            }
            None => {
                closed.tab = Some(tab.id);
                self.drop_tab(wi, ti, &mut closed);
            }
        }
        Ok(closed)
    }

    pub fn close_tab(&mut self, tab: TabId) -> Res<Closed> {
        let (wi, ti) = self.locate_tab(tab).ok_or(ModelError::NoTab)?;
        let panes = self.workspaces[wi].tabs[ti].layout.panes();
        for p in &panes {
            self.panes.remove(p);
        }
        let mut closed = Closed { panes, tab: Some(tab), workspace: None };
        self.drop_tab(wi, ti, &mut closed);
        Ok(closed)
    }

    fn drop_tab(&mut self, wi: usize, ti: usize, closed: &mut Closed) {
        let ws = &mut self.workspaces[wi];
        let gone = ws.tabs.remove(ti).id;
        if ws.tabs.is_empty() {
            closed.workspace = Some(ws.id);
            self.drop_workspace(wi);
        } else if ws.active_tab == gone {
            ws.active_tab = ws.tabs[ti.min(ws.tabs.len() - 1)].id;
        }
    }

    pub fn close_workspace(&mut self, workspace: WorkspaceId) -> Res<Closed> {
        let wi = self.workspaces.iter().position(|w| w.id == workspace).ok_or(ModelError::NoWorkspace)?;
        let panes: Vec<PaneId> = self.workspaces[wi].tabs.iter().flat_map(|t| t.layout.panes()).collect();
        for p in &panes {
            self.panes.remove(p);
        }
        self.drop_workspace(wi);
        Ok(Closed { panes, tab: None, workspace: Some(workspace) })
    }

    fn drop_workspace(&mut self, wi: usize) {
        let gone = self.workspaces.remove(wi).id;
        if self.active_workspace == Some(gone) {
            self.active_workspace =
                self.workspaces.get(wi.min(self.workspaces.len().saturating_sub(1))).map(|w| w.id);
        }
    }

    /// Focus a pane: its tab and workspace become the active ones, and a
    /// finished agent in it counts as seen.
    pub fn focus_pane(&mut self, pane: PaneId) -> Res<()> {
        let (wi, ti) = self.locate(pane).ok_or(ModelError::NoPane)?;
        let ws = &mut self.workspaces[wi];
        let tab = &mut ws.tabs[ti];
        if tab.focused != pane {
            tab.zoomed = false;
        }
        tab.focused = pane;
        ws.active_tab = tab.id;
        self.active_workspace = Some(ws.id);
        self.mark_seen(pane);
        Ok(())
    }

    pub fn select_tab(&mut self, tab: TabId) -> Res<()> {
        let (wi, ti) = self.locate_tab(tab).ok_or(ModelError::NoTab)?;
        let focused = self.workspaces[wi].tabs[ti].focused;
        self.focus_pane(focused)
    }

    pub fn select_workspace(&mut self, workspace: WorkspaceId) -> Res<()> {
        let ws = self.workspace(workspace).ok_or(ModelError::NoWorkspace)?;
        let tab = ws.active_tab;
        self.select_tab(tab)
    }

    /// Move focus to the pane on `side` of the focused pane of its tab.
    pub fn focus_side(&mut self, from: PaneId, side: Side) -> Res<Option<PaneId>> {
        let tab = self.tab_of_mut(from)?;
        let Some(next) = tab.layout.neighbor(from, side) else { return Ok(None) };
        self.focus_pane(next)?;
        Ok(Some(next))
    }

    pub fn swap_panes(&mut self, a: PaneId, b: PaneId) -> Res<()> {
        let tab = self.tab_of_mut(a)?;
        if tab.layout.swap(a, b) {
            Ok(())
        } else {
            Err(ModelError::Invalid("panes are not in the same tab"))
        }
    }

    pub fn set_ratio(&mut self, tab: TabId, path: &[u8], ratio: f32) -> Res<()> {
        let (wi, ti) = self.locate_tab(tab).ok_or(ModelError::NoTab)?;
        if self.workspaces[wi].tabs[ti].layout.set_ratio(path, ratio) {
            Ok(())
        } else {
            Err(ModelError::Invalid("no split at that path"))
        }
    }

    pub fn resize_pane(&mut self, pane: PaneId, side: Side, delta: f32) -> Res<bool> {
        Ok(self.tab_of_mut(pane)?.layout.resize(pane, side, delta))
    }

    pub fn toggle_zoom(&mut self, pane: PaneId) -> Res<bool> {
        self.focus_pane(pane)?;
        let tab = self.tab_of_mut(pane)?;
        tab.zoomed = !tab.zoomed && tab.layout.panes().len() > 1;
        Ok(tab.zoomed)
    }

    pub fn rename_pane(&mut self, pane: PaneId, name: String) -> Res<()> {
        self.panes.get_mut(&pane).ok_or(ModelError::NoPane)?.name = name;
        Ok(())
    }

    pub fn rename_tab(&mut self, tab: TabId, name: Option<String>) -> Res<()> {
        let (wi, ti) = self.locate_tab(tab).ok_or(ModelError::NoTab)?;
        self.workspaces[wi].tabs[ti].name = name.filter(|n| !n.trim().is_empty());
        Ok(())
    }

    pub fn rename_workspace(&mut self, workspace: WorkspaceId, name: String) -> Res<()> {
        self.workspace_mut(workspace)?.name = name;
        Ok(())
    }

    /// True when the pane is the one the user is looking at.
    pub fn is_in_view(&self, pane: PaneId) -> bool {
        self.active_workspace
            .and_then(|id| self.workspace(id))
            .and_then(|w| w.tabs.iter().find(|t| t.id == w.active_tab))
            .is_some_and(|t| if t.zoomed { t.focused == pane } else { t.layout.contains(pane) })
    }

    /// Apply what the detector saw. Returns true when the pane's state changed.
    ///
    /// An agent that stops while nobody is looking becomes `Done`, and stays
    /// `Done` until the pane is seen; that is the "come back to this one" mark.
    pub fn set_detected(&mut self, pane: PaneId, detected: Detected, window_visible: bool) -> bool {
        let seen = window_visible && self.is_in_view(pane);
        let Some(p) = self.panes.get_mut(&pane) else { return false };
        if p.kind == PaneKind::Shell || p.status == PaneStatus::Exited {
            return false;
        }
        let next = match detected {
            Detected::Blocked => AgentState::Blocked,
            Detected::Working => AgentState::Working,
            Detected::Idle => match p.agent {
                AgentState::Working | AgentState::Blocked if !seen => AgentState::Done,
                AgentState::Done if !seen => AgentState::Done,
                _ => AgentState::Idle,
            },
        };
        let changed = p.agent != next;
        p.agent = next;
        changed
    }

    /// The user looked at a pane: a `Done` mark on it is cleared.
    pub fn mark_seen(&mut self, pane: PaneId) -> bool {
        match self.panes.get_mut(&pane) {
            Some(p) if p.agent == AgentState::Done => {
                p.agent = AgentState::Idle;
                true
            }
            _ => false,
        }
    }

    pub fn set_exited(&mut self, pane: PaneId, code: Option<i32>) -> bool {
        let Some(p) = self.panes.get_mut(&pane) else { return false };
        p.status = PaneStatus::Exited;
        p.exit_code = code;
        p.agent = AgentState::Unknown;
        true
    }

    pub fn set_running(&mut self, pane: PaneId) -> bool {
        let Some(p) = self.panes.get_mut(&pane) else { return false };
        p.status = PaneStatus::Running;
        p.exit_code = None;
        p.agent = AgentState::Unknown;
        true
    }

    /// Highest-ranked agent state among the panes of a tab.
    pub fn tab_state(&self, tab: &Tab) -> AgentState {
        tab.layout
            .panes()
            .iter()
            .filter_map(|p| self.panes.get(p))
            .map(|p| p.agent)
            .max_by_key(|s| s.rank())
            .unwrap_or_default()
    }

    pub fn workspace_state(&self, workspace: &Workspace) -> AgentState {
        workspace.tabs.iter().map(|t| self.tab_state(t)).max_by_key(|s| s.rank()).unwrap_or_default()
    }

    /// Make a state read from disk safe to run on: nothing is running yet,
    /// and any structure that does not hold together is dropped.
    pub fn sanitize_after_load(&mut self) {
        for ws in &mut self.workspaces {
            ws.tabs.retain(|t| t.layout.is_valid() && t.layout.panes().iter().all(|p| self.panes.contains_key(p)));
            for tab in &mut ws.tabs {
                if !tab.layout.contains(tab.focused) {
                    tab.focused = tab.layout.panes()[0];
                }
            }
            if !ws.tabs.iter().any(|t| t.id == ws.active_tab) {
                if let Some(first) = ws.tabs.first() {
                    ws.active_tab = first.id;
                }
            }
        }
        self.workspaces.retain(|w| !w.tabs.is_empty());
        let placed: std::collections::HashSet<PaneId> =
            self.workspaces.iter().flat_map(|w| w.tabs.iter()).flat_map(|t| t.layout.panes()).collect();
        self.panes.retain(|id, _| placed.contains(id));
        for p in self.panes.values_mut() {
            p.status = PaneStatus::Exited;
            p.exit_code = None;
            p.agent = AgentState::Unknown;
        }
        if !self.active_workspace.is_some_and(|id| self.workspaces.iter().any(|w| w.id == id)) {
            self.active_workspace = self.workspaces.first().map(|w| w.id);
        }
        let max = self
            .workspaces
            .iter()
            .flat_map(|w| std::iter::once(w.id.0).chain(w.tabs.iter().map(|t| t.id.0)))
            .chain(self.panes.keys().map(|p| p.0))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max);
    }
}

/// Last path component, for naming a workspace after its folder.
pub fn folder_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut s: String = text.chars().take(max - 1).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> PaneSpec {
        PaneSpec::new(PaneKind::Claude)
    }

    fn state() -> (State, Created) {
        let mut s = State::default();
        let c = s.create_workspace(None, "C:\\work\\Nmux".into(), claude(), 1);
        (s, c)
    }

    #[test]
    fn workspace_is_named_after_its_folder() {
        let (s, c) = state();
        assert_eq!(s.workspace(c.workspace).unwrap().name, "Nmux");
        assert_eq!(s.panes[&c.pane].cwd, "C:\\work\\Nmux");
        assert_eq!(s.active_workspace, Some(c.workspace));
        assert_eq!(folder_name("/home/me/app/"), "app");
    }

    #[test]
    fn ids_are_never_shared() {
        let (mut s, c) = state();
        let t = s.create_tab(c.workspace, None, claude(), 2).unwrap();
        let p = s.split_pane(t.pane, Dir::Right, PaneSpec::new(PaneKind::Shell), 3).unwrap();
        let mut ids = vec![c.workspace.0, c.tab.0, c.pane.0, t.tab.0, t.pane.0, p.pane.0];
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn closing_the_last_pane_closes_tab_then_workspace() {
        let (mut s, c) = state();
        let split = s.split_pane(c.pane, Dir::Down, claude(), 2).unwrap();
        let closed = s.close_pane(split.pane).unwrap();
        assert_eq!(closed, Closed { panes: vec![split.pane], tab: None, workspace: None });
        assert_eq!(s.workspaces[0].tabs[0].focused, c.pane);

        let closed = s.close_pane(c.pane).unwrap();
        assert_eq!(closed.tab, Some(c.tab));
        assert_eq!(closed.workspace, Some(c.workspace));
        assert!(s.workspaces.is_empty() && s.panes.is_empty());
        assert_eq!(s.active_workspace, None);
    }

    #[test]
    fn closing_a_tab_moves_the_active_tab() {
        let (mut s, c) = state();
        let second = s.create_tab(c.workspace, Some("logs".into()), claude(), 2).unwrap();
        assert_eq!(s.workspaces[0].active_tab, second.tab);
        let closed = s.close_tab(second.tab).unwrap();
        assert_eq!(closed.panes, vec![second.pane]);
        assert_eq!(s.workspaces[0].active_tab, c.tab);
        assert_eq!(s.close_tab(second.tab), Err(ModelError::NoTab));
    }

    #[test]
    fn closing_a_workspace_activates_a_neighbour() {
        let (mut s, a) = state();
        let b = s.create_workspace(None, "C:\\work\\B".into(), claude(), 2);
        assert_eq!(s.active_workspace, Some(b.workspace));
        s.close_workspace(b.workspace).unwrap();
        assert_eq!(s.active_workspace, Some(a.workspace));
    }

    #[test]
    fn focus_follows_geometry_and_zoom_resets() {
        let (mut s, c) = state();
        let right = s.split_pane(c.pane, Dir::Right, claude(), 2).unwrap();
        assert!(s.toggle_zoom(right.pane).unwrap());
        assert_eq!(s.focus_side(right.pane, Side::Left).unwrap(), Some(c.pane));
        assert!(!s.workspaces[0].tabs[0].zoomed);
        assert_eq!(s.focus_side(c.pane, Side::Up).unwrap(), None);
    }

    #[test]
    fn a_single_pane_cannot_zoom() {
        let (mut s, c) = state();
        assert!(!s.toggle_zoom(c.pane).unwrap());
    }

    #[test]
    fn an_agent_that_stops_out_of_sight_is_done_until_seen() {
        let (mut s, a) = state();
        let b = s.create_tab(a.workspace, None, claude(), 2).unwrap();
        // `a` is on a background tab now.
        assert!(s.set_detected(a.pane, Detected::Working, true));
        assert!(s.set_detected(a.pane, Detected::Idle, true));
        assert_eq!(s.panes[&a.pane].agent, AgentState::Done);
        assert!(!s.set_detected(a.pane, Detected::Idle, true), "stays done");
        assert_eq!(s.workspace_state(&s.workspaces[0]), AgentState::Done);

        s.focus_pane(a.pane).unwrap();
        assert_eq!(s.panes[&a.pane].agent, AgentState::Idle);

        // In view: goes straight to idle. With the window hidden it does not.
        s.set_detected(a.pane, Detected::Working, true);
        s.set_detected(a.pane, Detected::Idle, true);
        assert_eq!(s.panes[&a.pane].agent, AgentState::Idle);
        s.set_detected(a.pane, Detected::Working, false);
        s.set_detected(a.pane, Detected::Idle, false);
        assert_eq!(s.panes[&a.pane].agent, AgentState::Done);
        let _ = b;
    }

    #[test]
    fn blocked_outranks_working_in_rollups() {
        let (mut s, a) = state();
        let b = s.split_pane(a.pane, Dir::Right, claude(), 2).unwrap();
        s.set_detected(a.pane, Detected::Working, true);
        s.set_detected(b.pane, Detected::Blocked, true);
        assert_eq!(s.tab_state(&s.workspaces[0].tabs[0]), AgentState::Blocked);
    }

    #[test]
    fn shells_never_get_an_agent_state() {
        let mut s = State::default();
        let c = s.create_workspace(None, "/w".into(), PaneSpec::new(PaneKind::Shell), 1);
        assert!(!s.set_detected(c.pane, Detected::Working, true));
        assert_eq!(s.panes[&c.pane].agent, AgentState::Unknown);
    }

    #[test]
    fn cloud_panes_are_named_after_their_task() {
        let mut s = State::default();
        let mut spec = PaneSpec::new(PaneKind::Cloud);
        spec.cloud_task = Some("Fix the flaky login test\nand tidy up".into());
        let c = s.create_workspace(None, "/w".into(), spec, 1);
        assert_eq!(s.panes[&c.pane].name, "Fix the flaky login test");
    }

    #[test]
    fn a_loaded_state_is_made_consistent() {
        let (mut s, c) = state();
        let other = s.create_tab(c.workspace, None, claude(), 2).unwrap();
        s.set_detected(c.pane, Detected::Working, true);
        // A tab pointing at a pane that is not in the map, and a stray pane.
        s.panes.remove(&other.pane);
        let stray = s.new_pane(claude(), "/x", 3);
        s.panes.insert(stray.id, stray);
        s.next_id = 0;

        let mut loaded: State = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        loaded.sanitize_after_load();
        assert_eq!(loaded.workspaces[0].tabs.len(), 1);
        assert_eq!(loaded.workspaces[0].active_tab, c.tab);
        assert_eq!(loaded.panes.len(), 1);
        assert_eq!(loaded.panes[&c.pane].status, PaneStatus::Exited);
        assert_eq!(loaded.panes[&c.pane].agent, AgentState::Unknown);
        assert!(loaded.next_id >= c.pane.0.max(c.tab.0).max(c.workspace.0));
    }
}
