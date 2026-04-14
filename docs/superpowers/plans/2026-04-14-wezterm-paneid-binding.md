# WezTerm pane_id Binding at SessionStart Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish `pid -> wezterm_pane_id` binding as early as possible by adding a `SessionStart` hook and applying a deterministic update policy for pane/session identifiers.

**Architecture:** Add `SessionStart` to the auto-configured pebble hooks so `pebble-bridge` fires on Claude session start. The bridge already detects `wezterm_pane_id` and sends it to the hook server. In the adapter, replace unconditional overwrites with an update policy that sets missing values, ignores duplicates, and refreshes on changes.

**Tech Stack:** Rust, Tauri, serde_json

---

### Task 1: Add `SessionStart` to Auto-Configured Hooks

**Files:**
- Modify: `pebble-app/src-tauri/src/hook/bridge.rs:71-78`

**Context:** The `pebble_hooks` JSON object lists all hook events that Pebble automatically registers in `~/.claude/settings.json`. Currently it lacks `SessionStart`, so `pebble-bridge` is never called when a new Claude session begins.

- [ ] **Step 1: Add `SessionStart` entry to `pebble_hooks`**

Insert `"SessionStart"` into the `pebble_hooks` map, after `"Stop"`:

```rust
let pebble_hooks = serde_json::json!({
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": format!("{} UserPromptSubmit", cmd) }] }],
    "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PreToolUse", cmd) }] }],
    "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUse", cmd) }] }],
    "PostToolUseFailure": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUseFailure", cmd) }] }],
    "PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PermissionRequest", cmd), "timeout": 300 }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": format!("{} Stop", cmd) }] }],
    "SessionStart": [{ "hooks": [{ "type": "command", "command": format!("{} SessionStart", cmd) }] }]
});
```

- [ ] **Step 2: Verify the change compiles**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: compilation succeeds with no new errors.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/hook/bridge.rs
git commit -m "feat(hook): add SessionStart to auto-configured hooks

Ensures wezterm_pane_id is sent to Pebble as soon as a Claude session starts.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Apply Deterministic Update Policy for Pane/Session IDs

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs:82-90`

**Context:** Currently `handle_hook` unconditionally overwrites `wezterm_pane_id`, `wt_session_id`, and `wezterm_unix_socket`. We want:
- `None` + incoming `Some(v)` → set to `v`
- `Some(old)` + incoming `Some(v)` where `old == v` → no-op
- `Some(old)` + incoming `Some(v)` where `old != v` → overwrite to `v`

- [ ] **Step 1: Add a private helper method for optional string updates**

In `impl ClaudeAdapter` block (near the bottom, around line 258), add:

```rust
/// Updates an optional string field using a deterministic policy:
/// - None + Some(v)      -> Set(v)
/// - Some(old) + Some(v) where old == v -> Ignore
/// - Some(old) + Some(v) where old != v -> Overwrite(v)
fn update_opt_string(current: &mut Option<String>, incoming: Option<&String>) {
    if let Some(v) = incoming {
        match current {
            Some(old) if old == v => {}
            _ => *current = Some(v.clone()),
        }
    }
}
```

- [ ] **Step 2: Replace direct assignments with helper calls**

Change the pane/session ID handling in `handle_hook` from:

```rust
if let Some(ref pane) = payload.wezterm_pane_id {
    state.wezterm_pane_id = Some(pane.clone());
}
if let Some(ref session) = payload.wt_session_id {
    state.wt_session_id = Some(session.clone());
}
if let Some(ref sock) = payload.wezterm_unix_socket {
    state.wezterm_unix_socket = Some(sock.clone());
}
```

To:

```rust
Self::update_opt_string(&mut state.wezterm_pane_id, payload.wezterm_pane_id.as_ref());
Self::update_opt_string(&mut state.wt_session_id, payload.wt_session_id.as_ref());
Self::update_opt_string(&mut state.wezterm_unix_socket, payload.wezterm_unix_socket.as_ref());
```

- [ ] **Step 3: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: compilation succeeds.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "feat(adapter): apply deterministic update policy for pane/session IDs

Sets missing values, ignores duplicates, and overwrites on changes so
SessionStart and later hooks coexist safely.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: End-to-End Verification

**Files:** None (manual verification)

- [ ] **Step 1: Build the Tauri app in dev mode**

```bash
cd pebble-app && npm run tauri dev
```

Wait until the Pebble window appears and the console shows `Finished dev profile`.

- [ ] **Step 2: Trigger auto-configuration (one-time)**

If Pebble auto-config is enabled, it will rewrite `~/.claude/settings.json` on startup. Verify the `SessionStart` hook is present:

```bash
grep -A 3 '"SessionStart"' ~/.claude/settings.json
```

Expected output contains a command ending with `pebble-bridge SessionStart`.

- [ ] **Step 3: Open a new Claude session in WezTerm**

In WezTerm, run:
```bash
claude
```

Do **not** type any message. Just wait.

- [ ] **Step 4: Check Pebble logs for the SessionStart hook**

In the terminal running `npm run tauri dev`, look for:
```
[pebble-hook ...] event=SessionStart ... pane=<number> ...
```

- [ ] **Step 5: Click the Pebble instance**

Click the newly discovered instance in the Pebble UI. It should perform a **pane-level jump** (the correct WezTerm pane/tab should be focused), not just a window-level activation.

- [ ] **Step 6: Stop the dev server**

Press `Ctrl+C` in the dev terminal.

---

## Self-Review

**1. Spec coverage:**
- `SessionStart` hook auto-configuration → Task 1
- Bridge pane_id detection on session start → Implicit (bridge logic is event-agnostic and already implemented)
- Adapter state update policy → Task 2
- Hook handler matching (no change needed) → Verified in Task 3
- No persistence / no polling fallback → Respected (not in plan)

**2. Placeholder scan:**
- No "TBD", "TODO", or vague steps.
- All code blocks contain complete, copy-pasteable snippets.
- Exact file paths and line ranges are specified.

**3. Type consistency:**
- `update_opt_string` signature uses `Option<String>` and `Option<&String>`, matching the existing `AdapterState` field types.
- Helper is called as `Self::update_opt_string`, consistent with it being an associated function on `ClaudeAdapter`.
