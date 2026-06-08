---
name: terminal-control
description: >
  Drive and observe terminal applications (TUIs / REPLs / interactive CLIs)
  inside herdr panes using the herdr JSON-RPC API (`pane.screen_text`,
  `pane.wait_for_text`, `pane.wait_for_idle`, `pane.send_input`,
  `pane.send_keys`, `pane.send_text`, `pane.split`, `workspace.create`).
  Use when an agent must operate or verify a TUI such as vim/nvim, lazygit,
  htop, k9s, fzf, claude-code, codex, or any shell-based interactive
  workflow inside a real herdr pane — particularly headless / remote /
  CI scenarios where there is no GUI.
  Triggers: "control vim", "drive a TUI", "automate terminal", "operate in
  pane", "agent control terminal", "headless terminal", "herdr 自动化",
  "herdr 控制终端".
---

# Herdr Terminal Control

Use **herdr's JSON-RPC API** to observe the actual visible terminal state
of a pane and drive interaction deterministically. Self-contained — no
`termctrl` binary required. Equivalent RPCs exist on the cmux app
(`surface.screen_text` etc.); use those when targeting a local cmux GUI
panel instead. The herdr path is the right choice for headless servers,
CI runners, and remote dev hosts where you have only a daemon, not a UI.

## Prerequisites

- A herdr daemon (started with `herdr` or `herdr server`).
- The API socket is at `~/.local/share/herdr/herdr-api.sock` by default
  (see `crate::session::active_api_socket_path`).
- All examples send line-delimited JSON requests over that Unix socket.
  The shell snippets below use a small helper:

  ```bash
  rpc() {
    printf '%s\n' "$1" | nc -U "$HERDR_API_SOCK"
  }
  export HERDR_API_SOCK="${HERDR_API_SOCK:-$HOME/.local/share/herdr/herdr-api.sock}"
  ```

## The Smallest Workflow

For "spawn a process in a fresh pane, wait until it settles, read the
screen":

```bash
# 1. Open a workspace + pane
RESP=$(rpc '{"id":"r1","method":"workspace.create","params":{"name":"work","cwd":"/tmp/work"}}')
PANE=$(echo "$RESP" | jq -r .result.root_pane.pane_id)

# 2. Run the program, wait until output stops, read the screen
rpc "{\"id\":\"r2\",\"method\":\"pane.send_input\",\"params\":{\"pane_id\":\"$PANE\",\"text\":\"my-terminal-app\\n\"}}"
rpc "{\"id\":\"r3\",\"method\":\"pane.wait_for_idle\",\"params\":{\"pane_id\":\"$PANE\",\"settle_ms\":400,\"deadline_ms\":5000}}"
rpc "{\"id\":\"r4\",\"method\":\"pane.screen_text\",\"params\":{\"pane_id\":\"$PANE\"}}"
```

For interactive/repeated inspection of a long-lived TUI:

```bash
PANE=...
rpc "{\"id\":\"a\",\"method\":\"pane.send_input\",\"params\":{\"pane_id\":\"$PANE\",\"text\":\"my-app\\n\"}}"
rpc "{\"id\":\"b\",\"method\":\"pane.wait_for_text\",\"params\":{\"pane_id\":\"$PANE\",\"match\":{\"type\":\"substring\",\"value\":\"Ready\"},\"timeout_ms\":5000}}"
rpc "{\"id\":\"c\",\"method\":\"pane.screen_text\",\"params\":{\"pane_id\":\"$PANE\"}}"
rpc "{\"id\":\"d\",\"method\":\"pane.send_input\",\"params\":{\"pane_id\":\"$PANE\",\"text\":\"help\\n\"}}"
rpc "{\"id\":\"e\",\"method\":\"pane.wait_for_text\",\"params\":{\"pane_id\":\"$PANE\",\"match\":{\"type\":\"substring\",\"value\":\"Commands\"},\"timeout_ms\":5000}}"
rpc "{\"id\":\"f\",\"method\":\"pane.screen_text\",\"params\":{\"pane_id\":\"$PANE\"}}"
```

## Choose The Correct Observation

| You want | Use |
|---|---|
| Current visible viewport text (alternate-screen TUI) | `pane.screen_text` |
| Scrollback text / persistent log output | `pane.read` |
| Wait until a string appears on visible screen | `pane.wait_for_text` |
| Wait until a string appears in scrollback / output stream | `pane.wait_for_output` |
| Wait until output settles (no new bytes for `settle_ms`) | `pane.wait_for_idle` |

> Do **not** treat `pane.read` as the visible state of an alternate-screen
> TUI like vim. `pane.screen_text` reads the libghostty-vt grid directly
> — that is the one source of truth for "what the user sees right now".

## Drive Input Precisely

`pane.send_input` writes raw bytes to the PTY. `pane.send_keys` sends
named keys (`enter`, `escape`, `ctrl+c`, `tab`, `up`, `down`, …).

```bash
PANE=...
# plain text + Enter
rpc "{\"id\":\"i1\",\"method\":\"pane.send_input\",\"params\":{\"pane_id\":\"$PANE\",\"text\":\"/connect\\n\"}}"

# arrow + Enter
rpc "{\"id\":\"i2\",\"method\":\"pane.send_keys\",\"params\":{\"pane_id\":\"$PANE\",\"keys\":[\"down\",\"enter\"]}}"

# Ctrl-C
rpc "{\"id\":\"i3\",\"method\":\"pane.send_keys\",\"params\":{\"pane_id\":\"$PANE\",\"keys\":[\"ctrl+c\"]}}"
```

JSON literal escapes inside the input payload work as you expect:
`\n` for newline, `` for Escape (used to leave vim insert mode),
`\t` for Tab, `` for Ctrl-C, etc.

> **Always `pane.wait_for_text` or `pane.wait_for_idle` after sending
> input.** Do not `sleep`. The whole point of these RPCs is to replace
> timing guesses with state-driven waits.

## The Vim/Nvim Pattern

Vim is the canonical "easy to break, hard to drive blindly" TUI. The
sequence below works reliably:

```bash
PANE=...
# 1. Launch vi from a clean shell prompt
rpc "{\"id\":\"v1\",\"method\":\"pane.send_input\",\"params\":{\"pane_id\":\"$PANE\",\"text\":\"vi /tmp/work/hello.txt\\n\"}}"

# 2. Wait until vi has finished drawing (status line shows the file name)
rpc "{\"id\":\"v2\",\"method\":\"pane.wait_for_text\",\"params\":{\"pane_id\":\"$PANE\",\"match\":{\"type\":\"substring\",\"value\":\"hello.txt\"},\"timeout_ms\":5000}}"
rpc "{\"id\":\"v3\",\"method\":\"pane.wait_for_idle\",\"params\":{\"pane_id\":\"$PANE\",\"settle_ms\":400,\"deadline_ms\":3000}}"

# 3. Atomic edit: i (insert) + content + ESC + :wq + Enter, single send
rpc "{\"id\":\"v4\",\"method\":\"pane.send_input\",\"params\":{\"pane_id\":\"$PANE\",\"text\":\"ihello from herdr agent\\nline 2\\nline 3\\u001b:wq\\n\"}}"

# 4. Wait for the shell prompt to return
rpc "{\"id\":\"v5\",\"method\":\"pane.wait_for_idle\",\"params\":{\"pane_id\":\"$PANE\",\"settle_ms\":400,\"deadline_ms\":3000}}"

# 5. Verify
cat /tmp/work/hello.txt
```

Why send the whole `i…:wq\n` block as one `pane.send_input`: vi's
mode-switch happens **inside** the buffered input, so a single byte
stream lets vi see the bytes in the same order regardless of round-trip
delay between RPC calls. Splitting `i` and the body across two RPCs
introduces races where keystrokes can be misclassified.

### Vim swap-file gotcha

If a previous session left a swap file behind, vi opens with a recovery
prompt instead of normal mode, and your `i` will be interpreted as
"answer ([O]/E/R/D/Q/A)". Always inspect:

```bash
rpc "{\"id\":\"x\",\"method\":\"pane.screen_text\",\"params\":{\"pane_id\":\"$PANE\"}}"
```

after launching vi. If you see `swap file ... already exists`, send `D`
(delete) or `R` (recover) before continuing — never assume vi is in
normal mode.

## Open A New Pane For The Task

```bash
# Split the current pane horizontally
RESP=$(rpc "{\"id\":\"s1\",\"method\":\"pane.split\",\"params\":{\"pane_id\":\"$PANE\",\"direction\":\"horizontal\"}}")
NEW_PANE=$(echo "$RESP" | jq -r .result.pane.pane_id)

# Or open a fresh workspace with its own working directory
RESP=$(rpc '{"id":"w1","method":"workspace.create","params":{"name":"task","cwd":"/path/to/cwd"}}')
NEW_PANE=$(echo "$RESP" | jq -r .result.root_pane.pane_id)
```

Use a **dedicated pane** for agent-driven work so any errant keystrokes
do not stomp on the user's terminal. A workspace gives you an isolated
shell with a clean cwd; a split keeps you in the user's workspace but in
a separate PTY.

## Targeting

Every observation/input RPC takes a `pane_id` (string identifier of the
form `<workspace_id>-<n>`). Always pass it explicitly — there is no
"focused pane" fallback over the API socket because herdr is meant to
be driven in parallel by multiple clients.

```json
{"pane_id": "w_1-1"}
```

## Subscribe vs Poll

Two ways to know "something changed":

- **Polling** (this skill): `pane.wait_for_text` and `pane.wait_for_idle`
  internally poll `pane.screen_text`. Simple, deterministic, fine for
  one-shot agent steps.
- **Streaming** (advanced): `events.subscribe` for layout / focus / pane
  metadata events. Use when you must react to user-driven changes (split
  closed, workspace switched) while the agent is mid-flight.

For most TUI control, polling is enough.

## Recover From Problems

| Symptom | Action |
|---|---|
| `pane.send_input` returns success but input never appears | The pane PTY is not running yet. `pane.wait_for_idle` first, then re-send. |
| Screen shows shell prompt instead of the program you launched | The launch command was eaten by a stuck program. Send `ctrl+c` (or `q` for some pagers), then `clear\n`, then re-launch. |
| `pane.wait_for_text` times out | Inspect with `pane.screen_text` to see what the pane actually shows; the substring may have wrapped or the program may be stuck on a confirmation prompt. |
| Vim writes content into the wrong buffer | Swap-file recovery prompt — see the swap-file gotcha above. |
| Connection hangs | Verify the herdr daemon is running; `ping` should return instantly. |

## Why This Beats Hooks Alone

Agent-CLI hooks (Claude Code's `PreToolUse`, Codex events) only fire
for events the upstream chose to expose. They cannot observe:

- Token-by-token streaming output (e.g. an LLM's mid-response thinking)
- TUI status bars, progress bars, modal dialogs
- Programs without any hook surface (vim, lazygit, htop, custom CLIs)

`pane.screen_text` works for **all** of those — it reads what the user
sees, not what the program was nice enough to broadcast.

## Independence From cmux / termctrl

These herdr RPCs are self-contained: herdr owns the PTY and reads its
libghostty-vt grid directly via `Terminal::visible_screen_text`. The
cmux app implements the same shape (`surface.screen_text` etc.) over
its own ghostty surface; use that for local GUI panels. **Never**
depend on the `termctrl` binary — herdr already provides everything
that skill exposes, with the added advantage of being headless-native.
