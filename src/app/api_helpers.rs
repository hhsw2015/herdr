pub(super) fn tab_attention_priority(state: crate::detect::AgentState, seen: bool) -> u8 {
    match (state, seen) {
        (crate::detect::AgentState::Blocked, _) => 4,
        (crate::detect::AgentState::Idle, false) => 3,
        (crate::detect::AgentState::Working, _) => 2,
        (crate::detect::AgentState::Idle, true) => 1,
        (crate::detect::AgentState::Unknown, _) => 0,
    }
}

/// Parse a `pane.send_keys` API string into a `crossterm::event::KeyEvent`.
///
/// Coverage targets full keyboard parity so agents can drive any TUI:
/// - Named keys: enter/return/tab/esc/escape/space/backspace/delete/insert
/// - Navigation: up/down/left/right/home/end/page_up/page_down
/// - Function keys: f1..f20
/// - Modifier prefixes: ctrl+, shift+, alt+, super+ (combinable, e.g. "ctrl+shift+a")
/// - Aliases for symbol characters: minus/equal/comma/period/slash/semicolon/quote/grave/backslash/left_bracket/right_bracket
/// - Single character literals (any UTF-8 char) — falls through to KeyCode::Char
///
/// Returns None for unrecognized input so the caller can surface a clear
/// `invalid_key` error instead of silently dropping.
fn parse_api_key(key: &str) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let normalized = key.trim();
    if normalized.is_empty() {
        return None;
    }

    // Strip modifier prefixes: ctrl+, shift+, alt+, super+ (case-insensitive,
    // also accepts shorthand c-/s-/a-/m-/cmd+/meta+). Any number of modifiers
    // may chain.
    let mut mods = KeyModifiers::empty();
    let mut rest = normalized.to_string();
    loop {
        let lower = rest.to_lowercase();
        let consume = |m: KeyModifiers, drop: usize| -> Option<(KeyModifiers, String)> {
            Some((m, rest[drop..].to_string()))
        };
        let stripped: Option<(KeyModifiers, String)> = if lower.starts_with("ctrl+") {
            consume(KeyModifiers::CONTROL, 5)
        } else if lower.starts_with("c-") {
            consume(KeyModifiers::CONTROL, 2)
        } else if lower.starts_with("shift+") {
            consume(KeyModifiers::SHIFT, 6)
        } else if lower.starts_with("s-") {
            consume(KeyModifiers::SHIFT, 2)
        } else if lower.starts_with("alt+") {
            consume(KeyModifiers::ALT, 4)
        } else if lower.starts_with("a-") {
            consume(KeyModifiers::ALT, 2)
        } else if lower.starts_with("super+") {
            consume(KeyModifiers::SUPER, 6)
        } else if lower.starts_with("meta+") {
            consume(KeyModifiers::SUPER, 5)
        } else if lower.starts_with("cmd+") {
            consume(KeyModifiers::SUPER, 4)
        } else {
            None
        };
        match stripped {
            Some((m, tail)) => {
                mods |= m;
                rest = tail;
            }
            None => break,
        }
    }
    let body = rest.to_lowercase();

    // Named keys (case-insensitive, multi-spelling tolerant).
    let code: KeyCode = match body.as_str() {
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" | "bksp" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "up" | "arrow_up" => KeyCode::Up,
        "down" | "arrow_down" => KeyCode::Down,
        "left" | "arrow_left" => KeyCode::Left,
        "right" | "arrow_right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "page_up" | "pageup" | "pgup" => KeyCode::PageUp,
        "page_down" | "pagedown" | "pgdn" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        "minus" | "dash" | "hyphen" => KeyCode::Char('-'),
        "equal" | "equals" => KeyCode::Char('='),
        "left_bracket" | "lbracket" | "leftbracket" => KeyCode::Char('['),
        "right_bracket" | "rbracket" | "rightbracket" => KeyCode::Char(']'),
        "semicolon" => KeyCode::Char(';'),
        "quote" | "apostrophe" => KeyCode::Char('\''),
        "comma" => KeyCode::Char(','),
        "period" | "dot" => KeyCode::Char('.'),
        "slash" => KeyCode::Char('/'),
        "grave" | "backtick" => KeyCode::Char('`'),
        "backslash" => KeyCode::Char('\\'),
        f if f.starts_with('f') => {
            if let Ok(n) = f[1..].parse::<u8>() {
                if (1..=20).contains(&n) {
                    KeyCode::F(n)
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
        // Single character literal (any UTF-8 char).
        ch if ch.chars().count() == 1 => KeyCode::Char(ch.chars().next().unwrap()),
        _ => return None,
    };
    Some(KeyEvent::new(code, mods))
}

pub(super) fn encode_api_text(runtime: &crate::terminal::TerminalRuntime, text: &str) -> Vec<u8> {
    let bracketed = runtime
        .input_state()
        .map(|state| state.bracketed_paste)
        .unwrap_or(false);
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.as_bytes().to_vec()
    }
}

pub(super) fn encode_api_keys(
    runtime: &crate::terminal::TerminalRuntime,
    keys: &[String],
) -> Result<Vec<Vec<u8>>, String> {
    let mut encoded_keys = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(key_event) = parse_api_key(key) else {
            return Err(key.clone());
        };
        encoded_keys.push(runtime.encode_terminal_key(key_event.into()));
    }
    Ok(encoded_keys)
}

pub(super) fn detect_state_from_api(
    state: crate::api::schema::PaneAgentState,
) -> crate::detect::AgentState {
    match state {
        crate::api::schema::PaneAgentState::Idle => crate::detect::AgentState::Idle,
        crate::api::schema::PaneAgentState::Working => crate::detect::AgentState::Working,
        crate::api::schema::PaneAgentState::Blocked => crate::detect::AgentState::Blocked,
        crate::api::schema::PaneAgentState::Unknown => crate::detect::AgentState::Unknown,
    }
}

pub(super) fn pane_agent_status(
    state: crate::detect::AgentState,
    seen: bool,
) -> crate::api::schema::AgentStatus {
    match (state, seen) {
        (crate::detect::AgentState::Idle, false) => crate::api::schema::AgentStatus::Done,
        (crate::detect::AgentState::Idle, true) => crate::api::schema::AgentStatus::Idle,
        (crate::detect::AgentState::Working, _) => crate::api::schema::AgentStatus::Working,
        (crate::detect::AgentState::Blocked, _) => crate::api::schema::AgentStatus::Blocked,
        (crate::detect::AgentState::Unknown, _) => crate::api::schema::AgentStatus::Unknown,
    }
}

pub(super) fn normalize_reported_agent_label(agent: &str) -> Option<String> {
    let trimmed = agent.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(agent) = crate::detect::parse_agent_label(trimmed) {
        return Some(crate::detect::agent_label(agent).to_string());
    }
    Some(trimmed.to_string())
}

pub(super) fn normalize_custom_status(status: Option<String>) -> Option<String> {
    let trimmed = status?.trim().to_string();
    let mut normalized = String::new();
    for ch in trimmed.chars().filter(|ch| !ch.is_control()).take(32) {
        normalized.push(ch);
    }
    (!normalized.trim().is_empty()).then(|| normalized.trim().to_string())
}
