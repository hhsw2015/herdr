//! Heuristic TUI state classifier shared by the `pane.tui_probe` RPC.
//! Mirrors `TuiStateClassifier` in `cmux/Sources/TerminalController.swift`
//! one-to-one — keep both implementations in sync when adjusting rules.
//!
//! Goal: let agents make routing decisions ("am I at a shell prompt
//! yet? is vim in insert mode?") on a 60-byte enum instead of fetching
//! and re-parsing the full grid every tick.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    pub kind: &'static str,
    pub indicators: Vec<String>,
}

/// Match vanilla vim ("All"/"Top"/"Bot" + "1,1" position) and
/// neovim with lualine/airline ("Top    1:1", "Bot 25:80", "12%"
/// patterns) statuslines. Cursor doesn't have to be on this line —
/// in nvim+lualine the cursor sits in the buffer area while statusline
/// renders below.
pub fn looks_like_vim_status_line(line: &str) -> bool {
    if line.len() < 4 {
        return false;
    }
    let has_position_word = line.contains("All") || line.contains("Top") || line.contains("Bot");
    let has_modified_flag = line.contains("[+]");
    // Coordinate: vim "1,1" or lualine "1:1"; word-boundary so we don't
    // catch "12,000 lines" or random colons in paths.
    let has_coordinate = line
        .split(|c: char| !c.is_ascii_digit() && c != ',' && c != ':')
        .any(|tok| {
            if let Some((a, b)) = tok.split_once([',', ':']) {
                !a.is_empty()
                    && !b.is_empty()
                    && a.chars().all(|c| c.is_ascii_digit())
                    && b.chars().all(|c| c.is_ascii_digit())
            } else {
                false
            }
        });
    // Percent like "12%" / "100%"
    let has_percent = line
        .split(|c: char| !c.is_ascii_digit())
        .any(|tok| !tok.is_empty())
        && line.contains('%');
    has_position_word || has_modified_flag || has_coordinate || has_percent
}

pub fn classify(rows: &[String], cursor_row: Option<u32>, cursor_col: Option<u32>) -> ProbeOutcome {
    let _ = cursor_col;
    let non_empty: Vec<&String> = rows.iter().filter(|r| !r.is_empty()).collect();
    let Some(last_ref) = non_empty.last() else {
        return ProbeOutcome {
            kind: "unknown",
            indicators: vec![],
        };
    };
    let last: &str = last_ref.as_str();
    let cursor_on_last = cursor_row
        .map(|r| (r as usize) + 1 == rows.len() || (r as usize) == rows.len().saturating_sub(1))
        .unwrap_or(false);

    if last.contains("-- INSERT --")
        || last.contains("-- VISUAL --")
        || last.contains("-- REPLACE --")
    {
        let mode = if last.contains("INSERT") {
            "vim_insert"
        } else if last.contains("VISUAL") {
            "vim_visual"
        } else {
            "vim_replace"
        };
        return ProbeOutcome {
            kind: mode,
            indicators: vec![last.to_string()],
        };
    }

    if cursor_on_last
        && (last.starts_with(':') || last.starts_with('/') || last.starts_with('?'))
        && last.len() <= 200
    {
        return ProbeOutcome {
            kind: "vim_command",
            indicators: vec![last.to_string()],
        };
    }

    if last.ends_with("(END)") || last == ":" {
        return ProbeOutcome {
            kind: "less_pager",
            indicators: vec![last.to_string()],
        };
    }
    if last.contains("--More--") || last.starts_with("Manual page") {
        return ProbeOutcome {
            kind: "less_pager",
            indicators: vec![last.to_string()],
        };
    }

    if cursor_on_last {
        let trimmed = last;
        if trimmed.ends_with("$ ")
            || trimmed.ends_with('$')
            || trimmed.ends_with("% ")
            || trimmed.ends_with('%')
            || trimmed.ends_with("# ")
            || trimmed.ends_with('#')
            || trimmed.ends_with("> ")
        {
            return ProbeOutcome {
                kind: "shell_prompt",
                indicators: vec![trimmed.to_string()],
            };
        }
        if trimmed.ends_with(">>> ") || trimmed.ends_with("In [") {
            return ProbeOutcome {
                kind: "repl_prompt",
                indicators: vec![trimmed.to_string()],
            };
        }
    }

    let lc_last = last.to_lowercase();
    if lc_last.contains("password:")
        || lc_last.contains("passphrase:")
        || lc_last.ends_with("? ")
        || lc_last.contains("(y/n)")
        || lc_last.contains("[y/n]")
        || lc_last.contains("(yes/no)")
    {
        return ProbeOutcome {
            kind: "input_prompt",
            indicators: vec![last.to_string()],
        };
    }

    if looks_like_vim_status_line(last) && last.len() > 4 {
        return ProbeOutcome {
            kind: "vim_normal",
            indicators: vec![last.to_string()],
        };
    }
    if rows.len() >= 2 {
        let second_last = &rows[rows.len() - 2];
        if looks_like_vim_status_line(second_last) && second_last.len() > 4 {
            return ProbeOutcome {
                kind: "vim_normal",
                indicators: vec![second_last.clone()],
            };
        }
        let tilde_rows = rows.iter().filter(|r| r.as_str() == "~").count();
        if tilde_rows >= 3 {
            return ProbeOutcome {
                kind: "vim_normal",
                indicators: vec!["tilde_buffer".to_string()],
            };
        }
    }

    if !cursor_on_last && non_empty.len() > 1 {
        return ProbeOutcome {
            kind: "running_command",
            indicators: vec![],
        };
    }
    ProbeOutcome {
        kind: "unknown",
        indicators: vec![last.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_prompt_dollar() {
        let out = classify(&[String::new(), "$ ".to_string()], Some(1), Some(2));
        assert_eq!(out.kind, "shell_prompt");
    }

    #[test]
    fn vim_insert_marker() {
        let out = classify(
            &[
                "hello".to_string(),
                "~".to_string(),
                "-- INSERT --".to_string(),
            ],
            Some(0),
            Some(5),
        );
        assert_eq!(out.kind, "vim_insert");
    }

    #[test]
    fn less_end_marker() {
        let out = classify(
            &[
                "line1".to_string(),
                "line2".to_string(),
                "(END)".to_string(),
            ],
            Some(2),
            Some(5),
        );
        assert_eq!(out.kind, "less_pager");
    }

    #[test]
    fn empty_rows_unknown() {
        let out = classify(&[], None, None);
        assert_eq!(out.kind, "unknown");
    }

    #[test]
    fn input_prompt_password() {
        let out = classify(&["Password: ".to_string()], Some(0), Some(10));
        assert_eq!(out.kind, "input_prompt");
    }

    #[test]
    fn nvim_lualine_top_position() {
        // Real-world neovim+lualine status line ("Top   1:1")
        let out = classify(
            &[
                String::new(),
                String::new(),
                " hello.txt  hello.txt                                       Top    1:1"
                    .to_string(),
            ],
            Some(0),
            Some(0),
        );
        assert_eq!(out.kind, "vim_normal");
    }

    #[test]
    fn lualine_bot_with_percent() {
        let out = classify(
            &[
                "buffer".to_string(),
                "~".to_string(),
                " file.py                                              Bot 47%".to_string(),
            ],
            Some(0),
            Some(0),
        );
        assert_eq!(out.kind, "vim_normal");
    }

    #[test]
    fn helper_recognizes_coordinate() {
        assert!(looks_like_vim_status_line(" file.txt  Top    1:1"));
        assert!(looks_like_vim_status_line(" file.py  Bot 25:80"));
        assert!(looks_like_vim_status_line(" 1,1     All"));
        assert!(!looks_like_vim_status_line("$ "));
        assert!(!looks_like_vim_status_line(""));
    }
}
