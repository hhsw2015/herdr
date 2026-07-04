use serde::{Deserialize, Serialize};

use super::agents::AgentInfo;
use super::common::{ClientWindowTitleReason, NotificationShowReason};
use super::events::EventEnvelope;
use super::integrations::{
    IntegrationInstallResult, IntegrationTarget, IntegrationUninstallResult,
};
use super::panes::{
    LayoutDescription, LayoutTree, PaneEdgesResult, PaneExpectErrorDetail, PaneExpectStepResult,
    PaneFocusDirectionResult, PaneInfo, PaneLayoutSnapshot, PaneMoveResult, PaneNeighborResult,
    PaneProcessInfo, PaneReadResult, PaneResizeResult, PaneScreenDiffRow, PaneSwapResult,
    PaneZoomResult,
};
use super::plugins::{
    InstalledPluginInfo, PluginActionInfo, PluginCommandLogInfo, PluginInvocationContext,
    PluginPaneInfo,
};
use super::server::ServerCapabilities;
use super::session::SessionSnapshot;
use super::tabs::TabInfo;
use super::workspaces::WorkspaceInfo;
use super::worktrees::{WorktreeInfo, WorktreeSourceInfo};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SuccessResponse {
    pub id: String,
    pub result: ResponseResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorResponse {
    pub id: String,
    pub error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseResult {
    Pong {
        version: String,
        protocol: u32,
        #[serde(default)]
        capabilities: Option<ServerCapabilities>,
    },
    SessionSnapshot {
        snapshot: Box<SessionSnapshot>,
    },
    WorkspaceInfo {
        workspace: WorkspaceInfo,
    },
    WorkspaceCreated {
        workspace: WorkspaceInfo,
        tab: TabInfo,
        root_pane: PaneInfo,
    },
    WorkspaceList {
        workspaces: Vec<WorkspaceInfo>,
    },
    WorktreeList {
        source: WorktreeSourceInfo,
        worktrees: Vec<WorktreeInfo>,
    },
    WorktreeCreated {
        workspace: WorkspaceInfo,
        tab: TabInfo,
        root_pane: PaneInfo,
        worktree: WorktreeInfo,
    },
    WorktreeOpened {
        workspace: WorkspaceInfo,
        tab: TabInfo,
        root_pane: PaneInfo,
        worktree: WorktreeInfo,
        already_open: bool,
    },
    WorktreeRemoved {
        workspace_id: String,
        path: String,
        forced: bool,
    },
    TabInfo {
        tab: TabInfo,
    },
    TabCreated {
        tab: TabInfo,
        root_pane: PaneInfo,
    },
    TabList {
        tabs: Vec<TabInfo>,
    },
    AgentInfo {
        agent: AgentInfo,
    },
    AgentStarted {
        agent: AgentInfo,
        argv: Vec<String>,
    },
    AgentList {
        agents: Vec<AgentInfo>,
    },
    PaneInfo {
        pane: PaneInfo,
    },
    PaneList {
        panes: Vec<PaneInfo>,
    },
    PaneCurrent {
        pane: PaneInfo,
    },
    PaneSwap {
        swap: PaneSwapResult,
    },
    PaneMove {
        move_result: PaneMoveResult,
    },
    PaneZoom {
        zoom: PaneZoomResult,
    },
    PaneLayout {
        layout: PaneLayoutSnapshot,
    },
    PaneProcessInfo {
        process_info: PaneProcessInfo,
    },
    LayoutExport {
        layout: LayoutDescription,
    },
    LayoutApply {
        layout: LayoutDescription,
    },
    LayoutSplitRatioSet {
        layout: LayoutDescription,
    },
    PaneNeighbor {
        neighbor: PaneNeighborResult,
    },
    PaneEdges {
        edges: PaneEdgesResult,
    },
    PaneFocusDirection {
        focus: PaneFocusDirectionResult,
    },
    PaneResize {
        resize: PaneResizeResult,
    },
    PaneRead {
        read: PaneReadResult,
    },
    AgentExplain {
        explain: serde_json::Value,
    },
    SubscriptionStarted {},
    WaitMatched {
        event: EventEnvelope,
    },
    OutputMatched {
        pane_id: String,
        revision: u64,
        matched_line: Option<String>,
        read: PaneReadResult,
    },
    NotificationShow {
        shown: bool,
        reason: NotificationShowReason,
    },
    ClientWindowTitle {
        changed: bool,
        reason: ClientWindowTitleReason,
    },
    IntegrationInstall {
        target: IntegrationTarget,
        details: IntegrationInstallResult,
    },
    IntegrationUninstall {
        target: IntegrationTarget,
        details: IntegrationUninstallResult,
    },
    AgentManifestReload {
        manifests: Vec<AgentManifestInfo>,
    },
    AgentManifestStatus {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_check_unix: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_result: Option<String>,
        manifests: Vec<AgentManifestInfo>,
    },
    PluginLinked {
        plugin: InstalledPluginInfo,
    },
    PluginList {
        plugins: Vec<InstalledPluginInfo>,
    },
    PluginUnlinked {
        plugin_id: String,
        removed: bool,
    },
    PluginEnabled {
        plugin: InstalledPluginInfo,
    },
    PluginDisabled {
        plugin: InstalledPluginInfo,
    },
    PluginActionList {
        actions: Vec<PluginActionInfo>,
    },
    PluginActionInvoked {
        action: PluginActionInfo,
        context: PluginInvocationContext,
        log: PluginCommandLogInfo,
    },
    PluginLogList {
        logs: Vec<PluginCommandLogInfo>,
    },
    PluginPaneOpened {
        plugin_pane: PluginPaneInfo,
    },
    PluginPaneFocused {
        plugin_pane: PluginPaneInfo,
    },
    PluginPaneClosed {
        pane_id: String,
    },
    ConfigReload {
        status: crate::config::ConfigReloadStatus,
        diagnostics: Vec<String>,
    },
    Ok {},
    LayoutSnapshot {
        tree: LayoutTree,
    },
    PaneScreenText {
        pane_id: String,
        text: String,
    },
    PaneScreenHash {
        pane_id: String,
        hash: String,
        rows: u16,
        columns: u16,
    },
    PaneScreenRegion {
        pane_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_rows: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_rows: Option<u32>,
    },
    PaneScreenDiff {
        pane_id: String,
        state_seq: u64,
        changed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        full: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt_screen_switched: Option<bool>,
        rows: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        dirty: Vec<PaneScreenDiffRow>,
    },
    PaneTuiProbe {
        pane_id: String,
        kind: String,
        rows: u32,
        columns: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor_row: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor_col: Option<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        indicators: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_line: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        last_lines: Vec<String>,
    },
    PaneKindMatched {
        pane_id: String,
        matched: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor_row: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor_col: Option<u32>,
    },
    PaneCursorMatched {
        pane_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor_row: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor_col: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
    PaneScreenChanged {
        pane_id: String,
        hash: String,
        changed: bool,
    },
    PaneExpect {
        pane_id: String,
        completed: u32,
        total: u32,
        steps: Vec<PaneExpectStepResult>,
        tail: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<PaneExpectErrorDetail>,
    },
    PaneTextMatched {
        pane_id: String,
        matched_line: Option<String>,
        text: String,
    },
    PaneIdle {
        pane_id: String,
        idle_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentManifestInfo {
    pub agent: String,
    pub source: String,
    pub source_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_remote_version: Option<String>,
    pub local_override_shadowing_remote: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_update_result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_update_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_last_checked_unix: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}
