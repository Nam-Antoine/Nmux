use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::layout::{Dir, Node, PaneId, Side};

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MachineId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MachineStatus {
    #[default]
    Unknown,
    Checking,
    Online,
    Auth,
    Error,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub id: MachineId,
    pub label: String,
    pub target: String,
    #[serde(default)]
    pub status: MachineStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub tmux: bool,
    #[serde(default)]
    pub claude: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TabId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneKind {
    Claude,
    Shell,
    Cloud,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneStatus {
    Running,
    Exited,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Blocked,
    Working,
    Done,
    Idle,
    #[default]
    Unknown,
}

impl AgentState {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teleported_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<MachineId>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub runs_claude: bool,
    pub created_at: u64,
}

impl Pane {
    pub fn shown_kind(&self) -> PaneKind {
        if self.runs_claude {
            PaneKind::Claude
        } else {
            self.kind
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneSpec {
    pub kind: PaneKind,
    #[serde(default)]
    pub name: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub layout: Node,
    pub focused: PaneId,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<MachineId>,
    #[serde(default)]
    pub number: u32,
}

impl Workspace {
    pub fn panes(&self) -> Vec<PaneId> {
        self.tabs.iter().flat_map(|t| t.layout.panes()).collect()
    }

    pub fn focused_pane(&self) -> Option<PaneId> {
        self.tabs.iter().find(|t| t.id == self.active_tab).or(self.tabs.first()).map(|t| t.focused)
    }
}

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub workspaces: Vec<Workspace>,
    #[serde(with = "panes_as_list")]
    pub panes: BTreeMap<PaneId, Pane>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_workspace: Option<WorkspaceId>,
    #[serde(default)]
    pub machines: Vec<Machine>,
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

#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Closed {
    pub panes: Vec<PaneId>,
    pub tab: Option<TabId>,
    pub workspace: Option<WorkspaceId>,
}

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

    fn new_pane(&mut self, spec: PaneSpec, workspace_cwd: &str, machine: Option<MachineId>, now: u64) -> Pane {
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
            machine,
            runs_claude: false,
            created_at: now,
        }
    }

    pub fn create_workspace(&mut self, name: Option<String>, cwd: String, spec: PaneSpec, now: u64) -> Created {
        self.create_workspace_on(None, name, cwd, spec, now)
    }

    pub fn create_remote_workspace(
        &mut self,
        machine: MachineId,
        name: Option<String>,
        cwd: String,
        spec: PaneSpec,
        now: u64,
    ) -> Res<Created> {
        if self.machine(machine).is_none() {
            return Err(ModelError::Invalid("no such machine"));
        }
        Ok(self.create_workspace_on(Some(machine), name, cwd, spec, now))
    }

    fn create_workspace_on(
        &mut self,
        machine: Option<MachineId>,
        name: Option<String>,
        cwd: String,
        spec: PaneSpec,
        now: u64,
    ) -> Created {
        let pane = self.new_pane(spec, &cwd, machine, now);
        let pane_id = pane.id;
        self.panes.insert(pane_id, pane);
        let tab = TabId(self.fresh_id());
        let workspace = WorkspaceId(self.fresh_id());
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| folder_name(&cwd));
        let number = self.free_number();
        self.workspaces.push(Workspace {
            number,
            id: workspace,
            name,
            cwd,
            tabs: vec![Tab { id: tab, name: None, layout: Node::leaf(pane_id), focused: pane_id, zoomed: false }],
            active_tab: tab,
            machine,
        });
        self.active_workspace = Some(workspace);
        Created { workspace, tab, pane: pane_id }
    }

    pub fn create_tab(&mut self, workspace: WorkspaceId, name: Option<String>, spec: PaneSpec, now: u64) -> Res<Created> {
        let ws = self.workspace(workspace).ok_or(ModelError::NoWorkspace)?;
        let (cwd, machine) = (ws.cwd.clone(), ws.machine);
        let pane = self.new_pane(spec, &cwd, machine, now);
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

    pub fn split_pane(&mut self, target: PaneId, dir: Dir, spec: PaneSpec, now: u64) -> Res<Created> {
        let (wi, ti) = self.locate(target).ok_or(ModelError::NoPane)?;
        let (cwd, machine) = (self.workspaces[wi].cwd.clone(), self.workspaces[wi].machine);
        let pane = self.new_pane(spec, &cwd, machine, now);
        let pane_id = pane.id;
        self.panes.insert(pane_id, pane);
        let ws = &mut self.workspaces[wi];
        let tab = &mut ws.tabs[ti];
        tab.layout.split(target, dir, pane_id);
        tab.focused = pane_id;
        tab.zoomed = false;
        Ok(Created { workspace: ws.id, tab: tab.id, pane: pane_id })
    }

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

    pub fn move_pane(&mut self, pane: PaneId, target: PaneId, side: Side) -> Res<()> {
        let tab = self.tab_of_mut(pane)?;
        if tab.layout.move_beside(pane, target, side) {
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

    pub fn is_in_view(&self, pane: PaneId) -> bool {
        self.active_workspace
            .and_then(|id| self.workspace(id))
            .and_then(|w| w.tabs.iter().find(|t| t.id == w.active_tab))
            .is_some_and(|t| if t.zoomed { t.focused == pane } else { t.layout.contains(pane) })
    }

    pub fn set_detected(&mut self, pane: PaneId, detected: Detected, window_visible: bool) -> bool {
        let seen = window_visible && self.is_in_view(pane);
        let Some(p) = self.panes.get_mut(&pane) else { return false };
        if p.shown_kind() == PaneKind::Shell || p.status == PaneStatus::Exited {
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
        p.runs_claude = false;
        true
    }

    pub fn set_running(&mut self, pane: PaneId) -> bool {
        let Some(p) = self.panes.get_mut(&pane) else { return false };
        p.status = PaneStatus::Running;
        p.exit_code = None;
        p.agent = AgentState::Unknown;
        p.runs_claude = false;
        true
    }

    pub fn set_runs_claude(&mut self, pane: PaneId, on: bool) -> bool {
        match self.panes.get_mut(&pane) {
            Some(p) if p.runs_claude != on && p.status == PaneStatus::Running => {
                p.runs_claude = on;
                p.agent = AgentState::Unknown;
                true
            }
            _ => false,
        }
    }

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

    pub fn machine(&self, id: MachineId) -> Option<&Machine> {
        self.machines.iter().find(|m| m.id == id)
    }

    pub fn add_machine(&mut self, label: String, target: String) -> Res<MachineId> {
        if self.machines.iter().any(|m| m.label.eq_ignore_ascii_case(&label)) {
            return Err(ModelError::Invalid("a machine with that name already exists"));
        }
        let id = MachineId(self.fresh_id());
        self.machines.push(Machine {
            id,
            label,
            target,
            status: MachineStatus::Unknown,
            detail: None,
            tmux: false,
            claude: false,
        });
        Ok(id)
    }

    pub fn rename_machine(&mut self, id: MachineId, label: String) -> Res<()> {
        if self.machines.iter().any(|m| m.id != id && m.label.eq_ignore_ascii_case(&label)) {
            return Err(ModelError::Invalid("a machine with that name already exists"));
        }
        let machine = self.machines.iter_mut().find(|m| m.id == id).ok_or(ModelError::Invalid("no such machine"))?;
        machine.label = label;
        Ok(())
    }

    pub fn remove_machine(&mut self, id: MachineId) -> Res<()> {
        if self.machine(id).is_none() {
            return Err(ModelError::Invalid("no such machine"));
        }
        if self.workspaces.iter().any(|w| w.machine == Some(id)) {
            return Err(ModelError::Invalid("close this machine's workspaces first"));
        }
        self.machines.retain(|m| m.id != id);
        Ok(())
    }

    pub fn set_machine_status(
        &mut self,
        id: MachineId,
        status: MachineStatus,
        detail: Option<String>,
        tmux: bool,
        claude: bool,
    ) -> bool {
        let Some(machine) = self.machines.iter_mut().find(|m| m.id == id) else { return false };
        let before = (machine.status, machine.detail.clone(), machine.tmux, machine.claude);
        machine.status = status;
        machine.detail = detail;
        if status == MachineStatus::Online {
            machine.tmux = tmux;
            machine.claude = claude;
        }
        before != (machine.status, machine.detail.clone(), machine.tmux, machine.claude)
    }

    fn free_number(&self) -> u32 {
        (1..).find(|n| self.workspaces.iter().all(|w| w.number != *n)).unwrap_or(1)
    }

    pub fn address(&self, pane: PaneId) -> Option<String> {
        self.workspaces.iter().find_map(|w| {
            let index = w.panes().iter().position(|p| *p == pane)?;
            let letter = if index == 0 { String::new() } else { ((b'a' + index.min(25) as u8) as char).to_string() };
            Some(format!("n{}{letter}", w.number))
        })
    }

    pub fn resolve(&self, address: &str) -> Result<PaneId, String> {
        let text = address.trim();
        let lower = text.to_lowercase();
        let numbered = lower.strip_prefix('n').and_then(|rest| {
            let digits = rest.trim_end_matches(|c: char| c.is_ascii_lowercase());
            let letter = &rest[digits.len()..];
            (letter.len() <= 1).then_some((digits.parse::<u32>().ok()?, letter.bytes().next().map_or(0, |l| (l - b'a') as usize)))
        });
        if let Some((number, index)) = numbered {
            let ws = self.workspaces.iter().find(|w| w.number == number).ok_or(format!("there is no workspace n{number}"))?;
            return ws.panes().get(index).copied().ok_or(format!("n{number} has no pane \"{text}\""));
        }
        let exact: Vec<&Workspace> = self.workspaces.iter().filter(|w| w.name.to_lowercase() == lower).collect();
        let found = if exact.is_empty() { self.workspaces.iter().filter(|w| w.name.to_lowercase().starts_with(&lower)).collect() } else { exact };
        match found.as_slice() {
            [ws] => ws.focused_pane().ok_or_else(|| "that workspace has no pane".to_string()),
            [] => Err(format!("nothing is called \"{text}\"; run: nc ls --all")),
            _ => Err(format!("more than one workspace matches \"{text}\"; use its n-number")),
        }
    }

    pub fn sanitize_after_load(&mut self) {
        for i in 0..self.workspaces.len() {
            let number = self.workspaces[i].number;
            if number == 0 || self.workspaces[..i].iter().any(|w| w.number == number) {
                self.workspaces[i].number = 0;
                self.workspaces[i].number = self.free_number();
            }
        }
        for machine in &mut self.machines {
            machine.status = MachineStatus::Unknown;
            machine.detail = None;
        }
        let known: std::collections::HashSet<MachineId> = self.machines.iter().map(|m| m.id).collect();
        self.workspaces.retain(|w| w.machine.is_none_or(|m| known.contains(&m)));
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
            p.runs_claude = false;
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
    fn panes_are_reached_by_workspace_number_or_name() {
        let (mut s, a) = state();
        let b = s.create_workspace(Some("TL education".into()), "/work/tl".into(), claude(), 2);
        let split = s.split_pane(b.pane, Dir::Right, PaneSpec::new(PaneKind::Shell), 3).unwrap();
        assert_eq!(s.address(a.pane).as_deref(), Some("n1"));
        assert_eq!(s.address(b.pane).as_deref(), Some("n2"));
        assert_eq!(s.address(split.pane).as_deref(), Some("n2b"));
        assert_eq!(s.resolve("n1"), Ok(a.pane));
        assert_eq!(s.resolve("N2"), Ok(b.pane));
        assert_eq!(s.resolve("n2a"), Ok(b.pane));
        assert_eq!(s.resolve("n2b"), Ok(split.pane));
        assert_eq!(s.resolve("tl edu"), Ok(split.pane));
        assert_eq!(s.resolve("nmux"), Ok(a.pane));
        assert!(s.resolve("n9").is_err() && s.resolve("n2e").is_err() && s.resolve("n2.2").is_err() && s.resolve("nothing").is_err());

        s.close_workspace(a.workspace).unwrap();
        assert_eq!(s.address(b.pane).as_deref(), Some("n2"));
        let c = s.create_workspace(None, "/work/new".into(), claude(), 4);
        assert_eq!(s.address(c.pane).as_deref(), Some("n1"));

        for w in &mut s.workspaces {
            w.number = 0;
        }
        s.sanitize_after_load();
        assert_eq!(s.workspaces.iter().map(|w| w.number).collect::<Vec<_>>(), vec![1, 2]);
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
        assert!(s.set_detected(a.pane, Detected::Working, true));
        assert!(s.set_detected(a.pane, Detected::Idle, true));
        assert_eq!(s.panes[&a.pane].agent, AgentState::Done);
        assert!(!s.set_detected(a.pane, Detected::Idle, true), "stays done");
        assert_eq!(s.workspace_state(&s.workspaces[0]), AgentState::Done);

        s.focus_pane(a.pane).unwrap();
        assert_eq!(s.panes[&a.pane].agent, AgentState::Idle);

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

        assert!(s.set_runs_claude(c.pane, true) && !s.set_runs_claude(c.pane, true));
        assert_eq!(s.panes[&c.pane].shown_kind(), PaneKind::Claude);
        assert!(s.set_detected(c.pane, Detected::Working, true));
        assert!(s.set_runs_claude(c.pane, false));
        assert_eq!((s.panes[&c.pane].shown_kind(), s.panes[&c.pane].agent), (PaneKind::Shell, AgentState::Unknown));
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
        s.panes.remove(&other.pane);
        let stray = s.new_pane(claude(), "/x", None, 3);
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
