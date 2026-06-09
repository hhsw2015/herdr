//! Batch step executor for `pane.expect`.
//!
//! Replaces N round trips with one. The connection thread runs each step
//! in order — sends go through `dispatch_to_app_with_timeout`, waits
//! reuse the existing `wait_for_text` / `wait_for_idle` helpers so the
//! abort-on-disconnect semantics stay identical to the standalone RPCs.
//!
//! Only the final response (completed step count, per-step status,
//! tail of the screen) is written back to the agent. Mirrors
//! `v2SurfaceExpect` in `cmux/Sources/TerminalController.swift`.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::api::schema::{
    Method, PaneExpectErrorDetail, PaneExpectParams, PaneExpectStep, PaneExpectStepResult,
    PaneSendKeysParams, PaneSendTextParams, PaneWaitForIdleParams, PaneWaitForTextParams, Request,
    ResponseResult, SuccessResponse,
};
use crate::api::server::{dispatch_to_app_with_timeout, APP_RESPONSE_TIMEOUT};
use crate::api::wait::{wait_for_idle, wait_for_text};
use crate::api::ApiRequestSender;
use crate::ipc::LocalStream;

pub(super) fn handle_expect(
    request_id: String,
    params: PaneExpectParams,
    stream: &mut LocalStream,
    api_tx: &ApiRequestSender,
    running: &Arc<AtomicBool>,
) -> std::io::Result<Option<String>> {
    let total = params.steps.len() as u32;
    let stop_on_error = params.stop_on_error;
    let tail_rows = params.tail_rows.unwrap_or(5);
    let pane_id = params.pane_id.clone();
    let mut step_results: Vec<PaneExpectStepResult> = Vec::with_capacity(params.steps.len());
    let mut completed: u32 = 0;
    let mut error_detail: Option<PaneExpectErrorDetail> = None;

    for (idx, step) in params.steps.into_iter().enumerate() {
        let step_id = format!("{request_id}:step{idx}");
        let outcome = match step {
            PaneExpectStep::Send { text } => dispatch_send(
                step_id,
                Method::PaneSendText(PaneSendTextParams {
                    pane_id: pane_id.clone(),
                    text,
                }),
                api_tx,
            ),
            PaneExpectStep::SendKey { key } => dispatch_send(
                step_id,
                Method::PaneSendKeys(PaneSendKeysParams {
                    pane_id: pane_id.clone(),
                    keys: vec![key],
                }),
                api_tx,
            ),
            PaneExpectStep::WaitText {
                r#match,
                timeout_ms,
            } => {
                match wait_for_text(
                    step_id.clone(),
                    PaneWaitForTextParams {
                        pane_id: pane_id.clone(),
                        r#match,
                        timeout_ms,
                    },
                    stream,
                    api_tx,
                    running,
                )? {
                    Some(resp) => parse_outcome(&resp),
                    None => return Ok(None),
                }
            }
            PaneExpectStep::WaitIdle {
                settle_ms,
                deadline_ms,
            } => match wait_for_idle(
                step_id.clone(),
                PaneWaitForIdleParams {
                    pane_id: pane_id.clone(),
                    settle_ms,
                    deadline_ms,
                },
                stream,
                api_tx,
                running,
            )? {
                Some(resp) => parse_outcome(&resp),
                None => return Ok(None),
            },
            PaneExpectStep::Sleep { sleep_ms } => {
                std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                StepOutcome::Ok
            }
        };

        match outcome {
            StepOutcome::Ok => {
                step_results.push(PaneExpectStepResult {
                    index: idx as u32,
                    ok: true,
                    code: None,
                    message: None,
                });
                completed += 1;
            }
            StepOutcome::Err { code, message } => {
                step_results.push(PaneExpectStepResult {
                    index: idx as u32,
                    ok: false,
                    code: Some(code.clone()),
                    message: Some(message.clone()),
                });
                error_detail = Some(PaneExpectErrorDetail {
                    index: Some(idx as u32),
                    code,
                    message,
                });
                if stop_on_error {
                    break;
                }
            }
        }
    }

    let tail = fetch_tail(&request_id, &pane_id, tail_rows, api_tx);

    let response = SuccessResponse {
        id: request_id,
        result: ResponseResult::PaneExpect {
            pane_id,
            completed,
            total,
            steps: step_results,
            tail,
            error: error_detail,
        },
    };
    Ok(Some(serde_json::to_string(&response).unwrap_or_else(
        |_| {
            r#"{"id":"","error":{"code":"internal_error","message":"failed to encode response"}}"#
                .to_string()
        },
    )))
}

enum StepOutcome {
    Ok,
    Err { code: String, message: String },
}

fn dispatch_send(step_id: String, method: Method, api_tx: &ApiRequestSender) -> StepOutcome {
    let req = Request {
        id: step_id,
        method,
    };
    let resp = dispatch_to_app_with_timeout(req, api_tx, Some(APP_RESPONSE_TIMEOUT));
    parse_outcome(&resp)
}

fn parse_outcome(response_json: &str) -> StepOutcome {
    let value: serde_json::Value = match serde_json::from_str(response_json) {
        Ok(v) => v,
        Err(err) => {
            return StepOutcome::Err {
                code: "invalid_response".into(),
                message: err.to_string(),
            };
        }
    };
    if let Some(err) = value.get("error") {
        let code = err
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("error")
            .to_string();
        let message = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return StepOutcome::Err { code, message };
    }
    StepOutcome::Ok
}

fn fetch_tail(
    request_id: &str,
    pane_id: &str,
    tail_rows: u32,
    api_tx: &ApiRequestSender,
) -> String {
    if tail_rows == 0 {
        return String::new();
    }
    let req = Request {
        id: format!("{request_id}:tail"),
        method: Method::PaneScreenRegion(crate::api::schema::PaneScreenRegionParams {
            pane_id: pane_id.to_string(),
            last_rows: Some(tail_rows),
            first_rows: None,
        }),
    };
    let resp = dispatch_to_app_with_timeout(req, api_tx, Some(APP_RESPONSE_TIMEOUT));
    let value: serde_json::Value = match serde_json::from_str(&resp) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    value["result"]["text"].as_str().unwrap_or("").to_string()
}
