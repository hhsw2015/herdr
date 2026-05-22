//! cmux-specific RPC handlers (PaneResize, LayoutSnapshot,
//! PaneSetSplitRatio, PaneSwap, PaneFocus, PaneSetZoom, TabReorder).
//! These were inlined in the giant `app/api.rs` match block before the
//! upstream refactor split api.rs into per-domain handler files. They
//! live here because they are not part of upstream herdr's core API
//! surface; they exist only for the cmux mirror integration.

use crate::api::schema::{
    EventData, EventEnvelope, EventKind, LayoutSnapshotParams, Method, PaneResizeParams,
    PaneSetSplitRatioParams, PaneSetZoomParams, PaneSwapParams, PaneTarget, ResponseResult,
    TabReorderParams,
};

use super::responses::{encode_error, encode_success};
use crate::app::state::Mode;
use crate::app::App;

fn pane_not_found(id: String, pane_id: &str) -> String {
    encode_error(id, "pane_not_found", format!("pane {pane_id} not found"))
}

fn tab_not_found(id: String, tab_id: &str) -> String {
    encode_error(id, "tab_not_found", format!("tab {tab_id} not found"))
}

fn workspace_not_found(id: String, workspace_id: &str) -> String {
    encode_error(
        id,
        "workspace_not_found",
        format!("workspace {workspace_id} not found"),
    )
}

impl App {
    pub(super) fn handle_pane_resize(&mut self, id: String, params: PaneResizeParams) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return pane_not_found(id, &params.pane_id);
        };
        let Some(runtime) = self.lookup_runtime_sender(ws_idx, pane_id) else {
            return encode_error(
                id,
                "pane_not_found",
                format!("pane {} runtime missing", params.pane_id),
            );
        };
        runtime.resize(
            params.rows,
            params.cols,
            params.cell_width_px,
            params.cell_height_px,
        );
        encode_success(id, ResponseResult::Ok {})
    }

    pub(super) fn handle_layout_snapshot(
        &mut self,
        id: String,
        params: LayoutSnapshotParams,
    ) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return tab_not_found(id, &params.tab_id);
        };
        let expected_workspace_id = self.public_workspace_id(ws_idx);
        if expected_workspace_id != params.workspace_id {
            return encode_error(
                id,
                "tab_not_found",
                format!(
                    "tab {} does not belong to workspace {}",
                    params.tab_id, params.workspace_id
                ),
            );
        }
        let Some(tree) = self.layout_tree(ws_idx, tab_idx) else {
            return tab_not_found(id, &params.tab_id);
        };
        encode_success(id, ResponseResult::LayoutSnapshot { tree })
    }

    pub(super) fn handle_pane_set_split_ratio(
        &mut self,
        id: String,
        params: PaneSetSplitRatioParams,
    ) -> String {
        let Some((ws_idx, tab_idx)) = self.parse_tab_id(&params.tab_id) else {
            return tab_not_found(id, &params.tab_id);
        };
        if self.public_workspace_id(ws_idx) != params.workspace_id {
            return encode_error(
                id,
                "tab_not_found",
                format!(
                    "tab {} does not belong to workspace {}",
                    params.tab_id, params.workspace_id
                ),
            );
        }
        let Some(tab) = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.tabs.get_mut(tab_idx))
        else {
            return tab_not_found(id, &params.tab_id);
        };
        if !tab.layout.set_ratio_at(&params.path, params.ratio) {
            return encode_error(
                id,
                "split_path_not_found",
                format!(
                    "no split at path {:?} in tab {}",
                    params.path, params.tab_id
                ),
            );
        }
        self.schedule_session_save();
        if let Some(tree) = self.layout_tree(ws_idx, tab_idx) {
            self.emit_event(EventEnvelope {
                event: EventKind::LayoutChanged,
                data: EventData::LayoutChanged { tree },
            });
        }
        encode_success(id, ResponseResult::Ok {})
    }

    pub(super) fn handle_pane_swap(&mut self, id: String, params: PaneSwapParams) -> String {
        let Some((ws_idx_a, pane_a)) = self.parse_pane_id(&params.a_pane_id) else {
            return pane_not_found(id, &params.a_pane_id);
        };
        let Some((ws_idx_b, pane_b)) = self.parse_pane_id(&params.b_pane_id) else {
            return pane_not_found(id, &params.b_pane_id);
        };
        if ws_idx_a != ws_idx_b {
            return encode_error(
                id,
                "swap_across_workspaces",
                "pane.swap requires both panes in the same workspace",
            );
        }
        let ws_idx = ws_idx_a;
        let Some(ws) = self.state.workspaces.get(ws_idx) else {
            return workspace_not_found(id, &ws_idx.to_string());
        };
        let Some(tab_idx_a) = ws.find_tab_index_for_pane(pane_a) else {
            return pane_not_found(id, &params.a_pane_id);
        };
        let Some(tab_idx_b) = ws.find_tab_index_for_pane(pane_b) else {
            return pane_not_found(id, &params.b_pane_id);
        };
        if tab_idx_a != tab_idx_b {
            return encode_error(
                id,
                "swap_across_tabs",
                "pane.swap requires both panes in the same tab",
            );
        }
        let tab_idx = tab_idx_a;
        let ok = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.tabs.get_mut(tab_idx))
            .map(|tab| tab.layout.swap_panes(pane_a, pane_b))
            .unwrap_or(false);
        if !ok {
            return encode_error(
                id,
                "pane_swap_failed",
                "swap rejected (panes identical or missing)",
            );
        }
        self.schedule_session_save();
        if let Some(tree) = self.layout_tree(ws_idx, tab_idx) {
            self.emit_event(EventEnvelope {
                event: EventKind::LayoutChanged,
                data: EventData::LayoutChanged { tree },
            });
        }
        encode_success(id, ResponseResult::Ok {})
    }

    pub(super) fn handle_pane_focus(&mut self, id: String, target: PaneTarget) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&target.pane_id) else {
            return pane_not_found(id, &target.pane_id);
        };
        let Some(tab_idx) = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|ws| ws.find_tab_index_for_pane(pane_id))
        else {
            return pane_not_found(id, &target.pane_id);
        };
        self.state.switch_workspace(ws_idx);
        self.state.switch_tab(tab_idx);
        let focused = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.tabs.get_mut(tab_idx))
            .map(|tab| tab.layout.focus_pane(pane_id))
            .unwrap_or(false);
        if !focused {
            return encode_error(
                id,
                "pane_not_found",
                format!("pane {} not found in tab", target.pane_id),
            );
        }
        self.state.mode = Mode::Terminal;
        self.sync_focus_events();
        let pane = self.pane_info(ws_idx, pane_id).unwrap();
        encode_success(id, ResponseResult::PaneInfo { pane })
    }

    pub(super) fn handle_pane_set_zoom(&mut self, id: String, params: PaneSetZoomParams) -> String {
        let Some((ws_idx, pane_id)) = self.parse_pane_id(&params.pane_id) else {
            return pane_not_found(id, &params.pane_id);
        };
        let Some(tab_idx) = self
            .state
            .workspaces
            .get(ws_idx)
            .and_then(|ws| ws.find_tab_index_for_pane(pane_id))
        else {
            return pane_not_found(id, &params.pane_id);
        };
        let workspace_id = self.public_workspace_id(ws_idx);
        let tab_id = format!("{}:{}", workspace_id, tab_idx + 1);
        if let Some(tab) = self
            .state
            .workspaces
            .get_mut(ws_idx)
            .and_then(|ws| ws.tabs.get_mut(tab_idx))
        {
            if tab.layout.pane_count() > 1 {
                tab.zoomed = params.zoomed;
                self.state.mark_session_dirty();
            }
        }
        self.emit_event(EventEnvelope {
            event: EventKind::PaneZoomed,
            data: EventData::PaneZoomed {
                workspace_id,
                tab_id,
                pane_id: params.pane_id.clone(),
                zoomed: params.zoomed,
            },
        });
        self.schedule_session_save();
        encode_success(id, ResponseResult::Ok {})
    }

    pub(super) fn handle_tab_reorder(&mut self, id: String, params: TabReorderParams) -> String {
        let Some(ws_idx) = self.parse_workspace_id(&params.workspace_id) else {
            return workspace_not_found(id, &params.workspace_id);
        };
        let Some(ws) = self.state.workspaces.get(ws_idx) else {
            return workspace_not_found(id, &params.workspace_id);
        };
        if params.tab_ids.len() != ws.tabs.len() {
            return encode_error(
                id,
                "tab_reorder_invalid",
                format!(
                    "expected {} tab ids, got {}",
                    ws.tabs.len(),
                    params.tab_ids.len()
                ),
            );
        }
        let mut permutation: Vec<usize> = Vec::with_capacity(params.tab_ids.len());
        for tab_id in &params.tab_ids {
            let Some((parsed_ws_idx, tab_idx)) = self.parse_tab_id(tab_id) else {
                return tab_not_found(id, tab_id);
            };
            if parsed_ws_idx != ws_idx {
                return encode_error(
                    id,
                    "tab_reorder_invalid",
                    format!(
                        "tab {} does not belong to workspace {}",
                        tab_id, params.workspace_id
                    ),
                );
            }
            if permutation.contains(&tab_idx) {
                return encode_error(
                    id,
                    "tab_reorder_invalid",
                    format!("tab {tab_id} appears more than once"),
                );
            }
            permutation.push(tab_idx);
        }
        let active_tab_id_before = self.state.workspaces[ws_idx]
            .tabs
            .get(self.state.workspaces[ws_idx].active_tab)
            .map(|t| t.root_pane);
        let ws = &mut self.state.workspaces[ws_idx];
        let mut reordered: Vec<crate::workspace::Tab> = Vec::with_capacity(ws.tabs.len());
        let mut taken: Vec<Option<crate::workspace::Tab>> = ws.tabs.drain(..).map(Some).collect();
        for idx in &permutation {
            if let Some(tab) = taken[*idx].take() {
                reordered.push(tab);
            }
        }
        ws.tabs = reordered;
        if let Some(root_pane) = active_tab_id_before {
            if let Some(new_idx) = ws.tabs.iter().position(|t| t.root_pane == root_pane) {
                ws.active_tab = new_idx;
            }
        }
        self.schedule_session_save();
        let new_tab_ids: Vec<String> = (0..self.state.workspaces[ws_idx].tabs.len())
            .filter_map(|idx| self.public_tab_id(ws_idx, idx))
            .collect();
        self.emit_event(EventEnvelope {
            event: EventKind::TabReordered,
            data: EventData::TabReordered {
                workspace_id: self.public_workspace_id(ws_idx),
                tab_ids: new_tab_ids,
            },
        });
        encode_success(id, ResponseResult::Ok {})
    }

    /// Inline dispatch helper for cmux methods. Called from the main
    /// `handle_api_request` match before falling through to
    /// not-implemented; returns Some(response) when a cmux handler
    /// was matched.
    /// Returns Some(response) when a cmux-only method matched.
    /// Returns None for any other method so the caller can fall through
    /// to upstream's not-implemented response. Unmatched method is
    /// dropped — caller doesn't need it back, and returning it inflates
    /// the Result enum past clippy's result-large-err threshold.
    pub(super) fn dispatch_cmux_method(&mut self, id: String, method: Method) -> Option<String> {
        match method {
            Method::PaneResize(params) => Some(self.handle_pane_resize(id, params)),
            Method::LayoutSnapshot(params) => Some(self.handle_layout_snapshot(id, params)),
            Method::PaneSetSplitRatio(params) => Some(self.handle_pane_set_split_ratio(id, params)),
            Method::PaneSwap(params) => Some(self.handle_pane_swap(id, params)),
            Method::PaneFocus(target) => Some(self.handle_pane_focus(id, target)),
            Method::PaneSetZoom(params) => Some(self.handle_pane_set_zoom(id, params)),
            Method::TabReorder(params) => Some(self.handle_tab_reorder(id, params)),
            _ => None,
        }
    }
}
