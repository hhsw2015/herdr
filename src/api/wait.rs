use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use regex::Regex;

use crate::api::schema::{
    ErrorBody, ErrorResponse, Method, OutputMatch, PaneTarget, PaneWaitForCursorParams,
    PaneWaitForIdleParams, PaneWaitForKindParams, PaneWaitForKindTarget,
    PaneWaitForScreenChangeParams, PaneWaitForTextParams, Request, ResponseResult, SuccessResponse,
};
use crate::api::server::{
    dispatch_to_app_with_timeout, should_stop_connection, APP_RESPONSE_TIMEOUT,
    CONNECTION_POLL_INTERVAL,
};
use crate::api::subscriptions::{match_output, output_match_read_source};
use crate::api::ApiRequestSender;
use crate::ipc::LocalStream;

pub(super) fn wait_for_output(
    request_id: String,
    params: crate::api::schema::PaneWaitForOutputParams,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    crate::logging::api_wait_started(&request_id, &params.pane_id, params.timeout_ms);
    let deadline = params
        .timeout_ms
        .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));

    let regex = match &params.r#match {
        crate::api::schema::OutputMatch::Regex { value } => match Regex::new(value) {
            Ok(regex) => Some(regex),
            Err(err) => {
                return Ok(Some(
                    serde_json::to_string(&ErrorResponse {
                        id: request_id,
                        error: ErrorBody {
                            code: "invalid_regex".into(),
                            message: err.to_string(),
                        },
                    })
                    .unwrap(),
                ));
            }
        },
        crate::api::schema::OutputMatch::Substring { .. } => None,
    };

    loop {
        if should_stop_connection(stream, running)? {
            crate::logging::api_wait_completed(&request_id, &params.pane_id, "client_disconnected");
            return Ok(None);
        }

        let read_request = Request {
            id: format!("{request_id}:read"),
            method: Method::PaneRead(crate::api::schema::PaneReadParams {
                pane_id: params.pane_id.clone(),
                source: output_match_read_source(&params.source),
                lines: params.lines,
                format: crate::api::schema::ReadFormat::Text,
                strip_ansi: params.strip_ansi,
            }),
        };
        let response =
            dispatch_to_app_with_timeout(read_request, api_tx, Some(APP_RESPONSE_TIMEOUT));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&response) else {
            return Ok(Some(response));
        };
        if value.get("error").is_some() {
            let mut value = value;
            value["id"] = serde_json::Value::String(request_id.clone());
            return Ok(Some(serde_json::to_string(&value).unwrap()));
        }

        let read_value = value["result"]["read"].clone();
        let Ok(read) = serde_json::from_value::<crate::api::schema::PaneReadResult>(read_value)
        else {
            return Ok(Some(
                serde_json::to_string(&ErrorResponse {
                    id: request_id,
                    error: ErrorBody {
                        code: "internal_error".into(),
                        message: "failed to decode pane read result".into(),
                    },
                })
                .unwrap(),
            ));
        };

        let matched_line = match_output(&read.text, &params.r#match, regex.as_ref());
        if matched_line.is_some() {
            let revision = read.revision;
            crate::logging::api_wait_completed(&request_id, &params.pane_id, "matched");
            return Ok(Some(
                serde_json::to_string(&SuccessResponse {
                    id: request_id,
                    result: ResponseResult::OutputMatched {
                        pane_id: read.pane_id.clone(),
                        revision,
                        matched_line,
                        read,
                    },
                })
                .unwrap(),
            ));
        }

        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            crate::logging::api_wait_timed_out(&request_id, &params.pane_id);
            return Ok(Some(
                serde_json::to_string(&ErrorResponse {
                    id: request_id,
                    error: ErrorBody {
                        code: "timeout".into(),
                        message: "timed out waiting for output match".into(),
                    },
                })
                .unwrap(),
            ));
        }

        std::thread::sleep(CONNECTION_POLL_INTERVAL);
    }
}

/// Block until the visible viewport contains a substring/regex match.
///
/// Polls `pane.screen_text` (libghostty-vt grid snapshot) every
/// `CONNECTION_POLL_INTERVAL`. Different from `wait_for_output` which polls
/// the scrollback ANSI stream. Self-contained — no termctrl/cmux deps.
pub(super) fn wait_for_text(
    request_id: String,
    params: PaneWaitForTextParams,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    crate::logging::api_wait_started(&request_id, &params.pane_id, params.timeout_ms);
    let deadline = params
        .timeout_ms
        .map(|ms| Instant::now() + Duration::from_millis(ms));

    let regex = match &params.r#match {
        OutputMatch::Regex { value } => match Regex::new(value) {
            Ok(regex) => Some(regex),
            Err(err) => {
                return Ok(Some(
                    serde_json::to_string(&ErrorResponse {
                        id: request_id,
                        error: ErrorBody {
                            code: "invalid_regex".into(),
                            message: err.to_string(),
                        },
                    })
                    .unwrap(),
                ));
            }
        },
        OutputMatch::Substring { .. } => None,
    };

    loop {
        if should_stop_connection(stream, running)? {
            crate::logging::api_wait_completed(&request_id, &params.pane_id, "client_disconnected");
            return Ok(None);
        }

        let screen_request = Request {
            id: format!("{request_id}:screen"),
            method: Method::PaneScreenText(PaneTarget {
                pane_id: params.pane_id.clone(),
            }),
        };
        let response =
            dispatch_to_app_with_timeout(screen_request, api_tx, Some(APP_RESPONSE_TIMEOUT));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&response) else {
            return Ok(Some(response));
        };
        if value.get("error").is_some() {
            let mut value = value;
            value["id"] = serde_json::Value::String(request_id.clone());
            return Ok(Some(serde_json::to_string(&value).unwrap()));
        }

        let text = value["result"]["text"].as_str().unwrap_or("").to_string();
        let matched_line = match_output(&text, &params.r#match, regex.as_ref());
        if matched_line.is_some() {
            crate::logging::api_wait_completed(&request_id, &params.pane_id, "matched");
            return Ok(Some(
                serde_json::to_string(&SuccessResponse {
                    id: request_id,
                    result: ResponseResult::PaneTextMatched {
                        pane_id: params.pane_id,
                        matched_line,
                        text,
                    },
                })
                .unwrap(),
            ));
        }

        if deadline.is_some_and(|d| Instant::now() >= d) {
            crate::logging::api_wait_timed_out(&request_id, &params.pane_id);
            return Ok(Some(
                serde_json::to_string(&ErrorResponse {
                    id: request_id,
                    error: ErrorBody {
                        code: "timeout".into(),
                        message: "timed out waiting for screen text match".into(),
                    },
                })
                .unwrap(),
            ));
        }

        std::thread::sleep(CONNECTION_POLL_INTERVAL);
    }
}

/// Block until the visible viewport stops changing for `settle_ms`.
///
/// Considered "idle" when two consecutive `pane.screen_text` snapshots taken
/// at least `settle_ms` apart are byte-identical. Returns `timeout` if
/// never settles before `deadline_ms`.
pub(super) fn wait_for_idle(
    request_id: String,
    params: PaneWaitForIdleParams,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    crate::logging::api_wait_started(&request_id, &params.pane_id, Some(params.deadline_ms));
    let deadline = Instant::now() + Duration::from_millis(params.deadline_ms);
    let settle = Duration::from_millis(params.settle_ms);
    let mut last_text: Option<String> = None;
    let mut stable_since: Option<Instant> = None;

    loop {
        if should_stop_connection(stream, running)? {
            crate::logging::api_wait_completed(&request_id, &params.pane_id, "client_disconnected");
            return Ok(None);
        }

        let screen_request = Request {
            id: format!("{request_id}:screen"),
            method: Method::PaneScreenText(PaneTarget {
                pane_id: params.pane_id.clone(),
            }),
        };
        let response =
            dispatch_to_app_with_timeout(screen_request, api_tx, Some(APP_RESPONSE_TIMEOUT));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&response) else {
            return Ok(Some(response));
        };
        if value.get("error").is_some() {
            let mut value = value;
            value["id"] = serde_json::Value::String(request_id.clone());
            return Ok(Some(serde_json::to_string(&value).unwrap()));
        }
        let text = value["result"]["text"].as_str().unwrap_or("").to_string();

        let now = Instant::now();
        match (&last_text, stable_since) {
            (Some(prev), Some(since)) if prev == &text => {
                if now.duration_since(since) >= settle {
                    let idle_ms = now.duration_since(since).as_millis() as u64;
                    crate::logging::api_wait_completed(&request_id, &params.pane_id, "idle");
                    return Ok(Some(
                        serde_json::to_string(&SuccessResponse {
                            id: request_id,
                            result: ResponseResult::PaneIdle {
                                pane_id: params.pane_id,
                                idle_ms,
                            },
                        })
                        .unwrap(),
                    ));
                }
            }
            _ => {
                stable_since = Some(now);
                last_text = Some(text);
            }
        }

        if now >= deadline {
            crate::logging::api_wait_timed_out(&request_id, &params.pane_id);
            return Ok(Some(
                serde_json::to_string(&ErrorResponse {
                    id: request_id,
                    error: ErrorBody {
                        code: "timeout".into(),
                        message: "timed out waiting for pane to go idle".into(),
                    },
                })
                .unwrap(),
            ));
        }

        std::thread::sleep(CONNECTION_POLL_INTERVAL);
    }
}

/// Block until `pane.tui_probe` reports a `kind` matching one of the
/// supplied targets. Polls every `CONNECTION_POLL_INTERVAL`. Used by
/// agents driving TUIs key-by-key — confirm `vi file` actually entered
/// vim_normal before sending `i`, etc.
pub(super) fn wait_for_kind(
    request_id: String,
    params: PaneWaitForKindParams,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    let timeout_ms = params.timeout_ms.unwrap_or(5_000);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let targets: std::collections::HashSet<String> = match &params.kind {
        PaneWaitForKindTarget::Single(s) => std::iter::once(s.clone()).collect(),
        PaneWaitForKindTarget::Many(v) => v.iter().cloned().collect(),
    };
    if targets.is_empty() {
        return Ok(Some(
            serde_json::to_string(&ErrorResponse {
                id: request_id,
                error: ErrorBody {
                    code: "invalid_params".into(),
                    message: "kind must contain at least one value".into(),
                },
            })
            .unwrap(),
        ));
    }

    loop {
        if should_stop_connection(stream, running)? {
            return Ok(None);
        }
        let probe_request = Request {
            id: format!("{request_id}:probe"),
            method: Method::PaneTuiProbe(PaneTarget {
                pane_id: params.pane_id.clone(),
            }),
        };
        let resp = dispatch_to_app_with_timeout(probe_request, api_tx, Some(APP_RESPONSE_TIMEOUT));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&resp) else {
            return Ok(Some(resp));
        };
        if value.get("error").is_some() {
            let mut value = value;
            value["id"] = serde_json::Value::String(request_id.clone());
            return Ok(Some(serde_json::to_string(&value).unwrap()));
        }
        let kind = value["result"]["kind"].as_str().unwrap_or("").to_string();
        if targets.contains(&kind) {
            let cursor_row = value["result"]["cursor_row"].as_u64().map(|v| v as u32);
            let cursor_col = value["result"]["cursor_col"].as_u64().map(|v| v as u32);
            return Ok(Some(
                serde_json::to_string(&SuccessResponse {
                    id: request_id,
                    result: ResponseResult::PaneKindMatched {
                        pane_id: params.pane_id,
                        matched: kind,
                        cursor_row,
                        cursor_col,
                    },
                })
                .unwrap(),
            ));
        }

        if Instant::now() >= deadline {
            return Ok(Some(
                serde_json::to_string(&ErrorResponse {
                    id: request_id,
                    error: ErrorBody {
                        code: "timeout".into(),
                        message: format!("timed out waiting for kind; last={kind}"),
                    },
                })
                .unwrap(),
            ));
        }
        std::thread::sleep(CONNECTION_POLL_INTERVAL);
    }
}

/// Block until cursor row/col/kind matches the target. Used to assert
/// post-keystroke positions ("after `gg`, cursor.row should be 0").
pub(super) fn wait_for_cursor(
    request_id: String,
    params: PaneWaitForCursorParams,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    if params.row.is_none() && params.col.is_none() && params.kind.is_none() {
        return Ok(Some(
            serde_json::to_string(&ErrorResponse {
                id: request_id,
                error: ErrorBody {
                    code: "invalid_params".into(),
                    message: "at least one of row/col/kind must be set".into(),
                },
            })
            .unwrap(),
        ));
    }
    let timeout_ms = params.timeout_ms.unwrap_or(5_000);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        if should_stop_connection(stream, running)? {
            return Ok(None);
        }
        let probe_request = Request {
            id: format!("{request_id}:probe"),
            method: Method::PaneTuiProbe(PaneTarget {
                pane_id: params.pane_id.clone(),
            }),
        };
        let resp = dispatch_to_app_with_timeout(probe_request, api_tx, Some(APP_RESPONSE_TIMEOUT));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&resp) else {
            return Ok(Some(resp));
        };
        if value.get("error").is_some() {
            let mut value = value;
            value["id"] = serde_json::Value::String(request_id.clone());
            return Ok(Some(serde_json::to_string(&value).unwrap()));
        }
        let cur_row = value["result"]["cursor_row"].as_u64().map(|v| v as u32);
        let cur_col = value["result"]["cursor_col"].as_u64().map(|v| v as u32);
        let cur_kind = value["result"]["kind"].as_str().map(|s| s.to_string());
        let row_ok = params.row.is_none() || cur_row == params.row;
        let col_ok = params.col.is_none() || cur_col == params.col;
        let kind_ok = params.kind.is_none() || cur_kind == params.kind;
        if row_ok && col_ok && kind_ok {
            return Ok(Some(
                serde_json::to_string(&SuccessResponse {
                    id: request_id,
                    result: ResponseResult::PaneCursorMatched {
                        pane_id: params.pane_id,
                        cursor_row: cur_row,
                        cursor_col: cur_col,
                        kind: cur_kind,
                    },
                })
                .unwrap(),
            ));
        }
        if Instant::now() >= deadline {
            return Ok(Some(
                serde_json::to_string(&ErrorResponse {
                    id: request_id,
                    error: ErrorBody {
                        code: "timeout".into(),
                        message: "timed out waiting for cursor".into(),
                    },
                })
                .unwrap(),
            ));
        }
        std::thread::sleep(CONNECTION_POLL_INTERVAL);
    }
}

/// Block until `pane.screen_hash` returns a digest different from
/// `prev_hash`. Provides a byte-level "did the keystroke land?" signal
/// without depending on classifier heuristics or cursor reporting.
/// Default timeout is 1500ms — fail-fast surfaces silently dropped
/// keys instantly. Polls at 25ms by default for sub-frame latency.
pub(super) fn wait_for_screen_change(
    request_id: String,
    params: PaneWaitForScreenChangeParams,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    let timeout_ms = params.timeout_ms.unwrap_or(1500);
    let poll_ms = params.poll_ms.unwrap_or(25);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let poll_interval = Duration::from_millis(poll_ms);
    loop {
        if should_stop_connection(stream, running)? {
            return Ok(None);
        }
        let hash_request = Request {
            id: format!("{request_id}:hash"),
            method: Method::PaneScreenHash(PaneTarget {
                pane_id: params.pane_id.clone(),
            }),
        };
        let resp = dispatch_to_app_with_timeout(hash_request, api_tx, Some(APP_RESPONSE_TIMEOUT));
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&resp) else {
            return Ok(Some(resp));
        };
        if value.get("error").is_some() {
            let mut value = value;
            value["id"] = serde_json::Value::String(request_id.clone());
            return Ok(Some(serde_json::to_string(&value).unwrap()));
        }
        let hash = value["result"]["hash"].as_str().unwrap_or("").to_string();
        if !hash.is_empty() && hash != params.prev_hash {
            return Ok(Some(
                serde_json::to_string(&SuccessResponse {
                    id: request_id,
                    result: ResponseResult::PaneScreenChanged {
                        pane_id: params.pane_id,
                        hash,
                        changed: true,
                    },
                })
                .unwrap(),
            ));
        }
        if Instant::now() >= deadline {
            return Ok(Some(
                serde_json::to_string(&ErrorResponse {
                    id: request_id,
                    error: ErrorBody {
                        code: "timeout".into(),
                        message: format!(
                            "screen did not change within {timeout_ms}ms — keystroke silently dropped (last_hash={hash})"
                        ),
                    },
                })
                .unwrap(),
            ));
        }
        std::thread::sleep(poll_interval);
    }
}
