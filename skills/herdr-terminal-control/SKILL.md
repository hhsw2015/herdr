---
name: herdr-terminal-control
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

## Token Budget (read first)

Each `pane.screen_text` returns the full visible grid (~2 KB for an
80×24 pane). Naive polling burns context fast. **Push waiting into the
daemon, read full screen only when you must.**

**Banned patterns** (do not write):

- `pane.screen_text` in a polling loop comparing against an expected
  substring. Use `pane.wait_for_text` — daemon polls internally,
  returns one bool.
- `pane.screen_text` followed by `sleep` followed by `pane.screen_text`.
  Use `pane.wait_for_idle` — daemon settles for you.
- `pane.screen_text` *just to check* if the screen changed.
  Use `pane.screen_hash` — returns 32-byte digest + seq, ~50 byte
  response.
- Reading the full grid when you only care about the last line
  (shell prompt, vim status bar, less footer).
  Use `pane.screen_region` with `last_rows`.

**Recommended pattern**:

1. Send input → `wait_for_text` (or `wait_for_idle`) — never `sleep`.
2. Cache previous `screen_hash`. On each tick, ask for the hash; if
   unchanged, do nothing. Only fetch full text when hash differs.
3. Read full `screen_text` only at decision points or for final
   verification, not every iteration.
4. For multi-step scripted flows, use `pane.expect` (one RPC, N steps
   inside the daemon) instead of N round trips.

**Rule of thumb**: an automated TUI session should average
≤ 200 bytes of RPC response per agent step. If you are reading a full
screen grid every step, the loop is wrong.

## Generic TUI Categories

Every interactive terminal program falls into one of these shapes; pick
the right primitive accordingly:

| Shape | Examples | Wait primitive | Read primitive |
|---|---|---|---|
| **Line-based REPL / shell** | bash, zsh, python, ipython, psql | `wait_for_text` on prompt regex (`\$ $` / `>>> `) | `screen_region {last_rows: 5}` |
| **Full-screen modal** | vim, less, man, k9s | `wait_for_text` on status-line marker (`-- INSERT --`, `(END)`) | `screen_region {last_rows: 1}` for status, `screen_text` for body |
| **Menu / picker** | fzf, lazygit, gh, htop | `wait_for_idle` after arrow keys | `screen_text` once, `screen_hash` between keystrokes |
| **Input prompt / confirmation** | sudo, ssh password, `(y/n)` | `wait_for_text` on the literal prompt string | none — send and `wait_for_idle` |
| **Long-running stream** | docker logs, kubectl logs, tail -f | `wait_for_text` on a known marker line | `screen_region {last_rows: 10}` |

When unsure: probe with `pane.screen_region {last_rows: 3}` first.
A single line tells you 80% of the time which category you are in.

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

| You want | Use | Typical bytes |
|---|---|---|
| Current visible viewport text (alt-screen TUI) | `pane.screen_text` | ~2 KB |
| Just the bottom N rows (prompt / status line) | `pane.screen_region {last_rows: N}` | ~80 B/row |
| Just changed rows since last seq | `pane.screen_diff {since_seq}` | 30-200 B |
| Did the screen change since last poll? | `pane.screen_hash` | ~100 B |
| What kind of TUI is this? | `pane.tui_probe` | ~150 B |
| Scrollback text / persistent log output | `pane.read` | varies |
| Wait until a string appears on visible screen | `pane.wait_for_text` | ~50 B |
| Wait until a string appears in scrollback | `pane.wait_for_output` | ~50 B |
| Wait until output settles | `pane.wait_for_idle` | ~50 B |
| Run a multi-step send/wait flow in one round trip | `pane.expect` | ~200 B (final only) |

> Do **not** treat `pane.read` as the visible state of an alternate-screen
> TUI like vim. `pane.screen_text` reads the libghostty-vt grid directly
> — that is the one source of truth for "what the user sees right now".

### Long-Running Loops: Use `screen_diff`, Not `screen_text`

For agents that read streamed output (LLM completion, `tail -f`,
`docker logs`), call `pane.screen_diff` with the previous `state_seq`
each tick. First call (`since_seq` omitted/0) returns full text + a
seq; subsequent calls with that seq return only changed rows or a
`{changed:false}` no-op (~30 B). The daemon falls back to a full
snapshot when >60% of rows changed or the alt-screen toggled.

A 1000-iteration polling loop on an idle pane costs ~30 KB total with
`screen_diff` vs ~2 MB with `screen_text`. Same precision (you see every
character that hits the screen), 60× cheaper.

### Routing Decisions: Use `tui_probe`, Not `screen_text` Parsing

If your agent needs to know "am I at a shell prompt yet?" or "is vim in
insert mode?", call `pane.tui_probe` and switch on the `kind` field.
Possible values: `shell_prompt`, `repl_prompt`, `vim_normal`,
`vim_insert`, `vim_visual`, `vim_replace`, `vim_command`, `less_pager`,
`input_prompt`, `running_command`, `unknown`. Always have a fallback
for `unknown` — the classifier is heuristic.

### Multi-Step Flows: Use `pane.expect`, Not Loops

```json
{
  "id": "exp1",
  "method": "pane.expect",
  "params": {
    "pane_id": "...",
    "steps": [
      {"verb": "send", "text": "ls\n"},
      {"verb": "wait_text", "match": {"type": "substring", "value": "$ "}, "timeout_ms": 3000},
      {"verb": "send", "text": "cd /tmp\n"},
      {"verb": "wait_text", "match": {"type": "substring", "value": "$ "}, "timeout_ms": 3000}
    ],
    "stop_on_error": true,
    "tail_rows": 5
  }
}
```

Returns `{completed, total, steps[], tail, error?}`. Four steps in one
RPC instead of four RPCs + four waits. Saves ~70% over the naive loop;
the daemon never returns until the whole sequence finishes or one step
fails with `stop_on_error: true`.

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
