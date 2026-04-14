# WezTerm pane_id Binding at Session Start

## Problem

Currently, `wezterm_pane_id` is only sent to the Pebble server when the first `UserPromptSubmit` hook fires. This means if the user opens a Claude session in WezTerm and does not interact with it (no chat, no tool use), clicking the Pebble instance will fall back to window-level activation instead of precise pane-level jumping.

iTerm2 does not have this problem because it uses TTY-based AppleScript activation, which can be resolved dynamically from the pid alone. WezTerm requires a stable `pane_id` from the environment or CLI.

## Goal

Bind `pid -> wezterm_pane_id` as early as possible (at Claude session start) so that precise WezTerm pane navigation works immediately, without waiting for the first user interaction.

## Design

### 1. Add `SessionStart` Hook Auto-Configuration

In `hook/bridge.rs`, add `"SessionStart"` to the auto-generated hooks configuration so that `pebble-bridge` is invoked when a new Claude session starts.

```json
"SessionStart": [
  { "hooks": [{ "type": "command", "command": "<bridge> SessionStart" }] }
]
```

### 2. Bridge Already Supports pane_id Detection

`bin/pebble-bridge.rs` currently:
- Reads `WEZTERM_PANE` env var
- Falls back to `detect_wezterm_pane(cwd)` via `wezterm cli list --format json`
- Detects `wezterm_unix_socket` via `detect_wezterm_socket()`

This logic is event-agnostic and runs for every hook event. Adding `SessionStart` means the bridge will automatically send `wezterm_pane_id`, `wezterm_unix_socket`, and `wt_session_id` on session start. No changes to `pebble-bridge.rs` core logic are required.

### 3. Adapter State pane_id Update Policy

In `adapter/claude.rs`, enhance `handle_hook` to apply a deterministic update policy for pane/session identifiers:

| Current State | Incoming Value | Action |
|---------------|----------------|--------|
| `None` | any | Set to incoming value |
| `Some(old)` | same as `old` | Ignore (no-op) |
| `Some(old)` | different from `old` | Overwrite with new value |

This policy applies to:
- `wezterm_pane_id`
- `wezterm_unix_socket`
- `wt_session_id`

This supports:
- Early binding via `SessionStart`
- Late binding via `UserPromptSubmit` if `SessionStart` missed it
- Re-binding if the user moved the Claude session to a new WezTerm pane

### 4. Hook Handler Matching

`main.rs` hook handler already matches incoming payloads by `sender_pid` (`cc-{pid}`) and `cwd`. `SessionStart` payloads include `sender_pid` injected by the bridge, so the match and state update will work without modification.

## Out of Scope

- **Persistence**: pane_id binding remains in-memory only. If Pebble restarts, it must be re-acquired via `SessionStart` or subsequent hooks.
- **Polling fallback**: Active pane detection via `wezterm cli` inside the 1-second state monitor loop is deferred to a future iteration if needed.
- **Other terminals**: No change to iTerm2, Terminal.app, or Windows Terminal behavior.

## Files to Modify

1. `pebble-app/src-tauri/src/hook/bridge.rs` — add `SessionStart` hook to auto-config
2. `pebble-app/src-tauri/src/adapter/claude.rs` — add paneid update policy in `handle_hook`
