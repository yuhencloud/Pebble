# Windows Terminal Tab Jump (Plan A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture terminal environment variables (`WEZTERM_PANE`, `WT_SESSION`) from the bridge and use them to activate the exact WezTerm pane/tab on Windows, while keeping Windows Terminal and other terminals on window-level fallback.

**Architecture:** Extend the hook payload with terminal IDs, persist them into instance state, and dispatch `jump_to_terminal` by `terminal_app` — using `wezterm cli activate-tab` for WezTerm and keeping existing `SetForegroundWindow` for others.

**Tech Stack:** Rust (Tauri backend), `std::process::Command`, `sysinfo`, existing `pebble-bridge` TCP hook bridge.

---

## File Structure

| File | Responsibility |
|------|----------------|
| `pebble-app/src-tauri/src/bin/pebble-bridge.rs` | Bridge executable: reads env vars `WEZTERM_PANE`, `WT_SESSION`, includes them in POST payload |
| `pebble-app/src-tauri/src/types.rs` | Shared types: add `wezterm_pane_id` and `wt_session_id` to `IncomingHookPayload`, `HookPayload`, `AdapterState`, `Instance` |
| `pebble-app/src-tauri/src/adapter/mod.rs` | Update `HookPayload` struct definition |
| `pebble-app/src-tauri/src/adapter/claude.rs` | `ClaudeAdapter::handle_hook` persists new IDs; `discover_instances` tries to read env vars from process environment via `/proc` equivalent or session files |
| `pebble-app/src-tauri/src/platform/jump.rs` | Add terminal-specific dispatch (`activate_wezterm_pane`, `activate_windows_terminal_session`) and wire into `jump_to_terminal` |
| `pebble-app/src-tauri/src/platform/terminal.rs` | No changes needed (already identifies terminal apps) |
| `pebble-app/src-tauri/src/session.rs` | Optional: extend `SessionInfo` if we want to cache pane/session IDs there |
| `pebble-app/src-tauri/src/main.rs` | Maps `IncomingHookPayload` → `HookPayload`, ensure new fields are forwarded |

---

## Task 1: Extend Hook Payload Types

**Files:**
- Modify: `pebble-app/src-tauri/src/types.rs`
- Modify: `pebble-app/src-tauri/src/adapter/mod.rs`

- [ ] **Step 1: Add `wezterm_pane_id` and `wt_session_id` to `IncomingHookPayload`**

```rust
// In pebble-app/src-tauri/src/types.rs, inside IncomingHookPayload:
#[serde(default, rename = "wezterm_pane_id")]
pub wezterm_pane_id: Option<String>,
#[serde(default, rename = "wt_session_id")]
pub wt_session_id: Option<String>,
```

- [ ] **Step 2: Add the same two fields to `HookPayload` in `adapter/mod.rs`**

```rust
pub struct HookPayload {
    pub event: String,
    pub cwd: String,
    pub timestamp: u64,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub permission_mode: Option<String>,
    pub tool_use_id: Option<String>,
    pub model: Option<String>,
    pub context_percent: Option<u8>,
    pub session_name: Option<String>,
    pub transcript_path: Option<String>,
    pub choices: Option<Vec<String>>,
    pub default_choice: Option<String>,
    pub wezterm_pane_id: Option<String>,
    pub wt_session_id: Option<String>,
}
```

- [ ] **Step 3: Add the same two fields to `AdapterState` in `adapter/mod.rs`**

```rust
#[derive(Debug, Clone, Default)]
pub struct AdapterState {
    pub status: String,
    pub last_activity: u64,
    pub last_hook_event: Option<crate::types::HookEvent>,
    pub pending_permission: Option<crate::types::PendingPermission>,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub context_percent: Option<u8>,
    pub conversation_log: Vec<String>,
    pub session_start: Option<u64>,
    pub transcript_path: Option<String>,
    pub session_name: Option<String>,
    pub wezterm_pane_id: Option<String>,
    pub wt_session_id: Option<String>,
}
```

- [ ] **Step 4: Add the same two fields to `Instance` in `types.rs`**

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub wezterm_pane_id: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
pub wt_session_id: Option<String>,
```

- [ ] **Step 5: Run `cargo check` to verify no type errors**

Run: `cd pebble-app/src-tauri && cargo check`
Expected: PASS (may warn about unused fields, which is fine)

- [ ] **Step 6: Commit**

```bash
git add pebble-app/src-tauri/src/types.rs pebble-app/src-tauri/src/adapter/mod.rs
git commit -m "types: add wezterm_pane_id and wt_session_id to payloads and state"
```

---

## Task 2: Update pebble-bridge to Capture Env Vars

**Files:**
- Modify: `pebble-app/src-tauri/src/bin/pebble-bridge.rs`

- [ ] **Step 1: Read `WEZTERM_PANE` and `WT_SESSION` from environment**

After building the base `body` JSON object (around line 49), add:

```rust
    if let Some(pane) = std::env::var("WEZTERM_PANE").ok() {
        if !pane.trim().is_empty() {
            body["wezterm_pane_id"] = serde_json::json!(pane.trim());
        }
    }
    if let Some(session) = std::env::var("WT_SESSION").ok() {
        if !session.trim().is_empty() {
            body["wt_session_id"] = serde_json::json!(session.trim());
        }
    }
```

- [ ] **Step 2: Run `cargo check`**

Run: `cd pebble-app/src-tauri && cargo check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/bin/pebble-bridge.rs
git commit -m "bridge: capture WEZTERM_PANE and WT_SESSION env vars in payload"
```

---

## Task 3: Forward New Fields in main.rs Hook Mapping

**Files:**
- Modify: `pebble-app/src-tauri/src/main.rs`

- [ ] **Step 1: Forward `wezterm_pane_id` and `wt_session_id` when building `HookPayload`**

Inside `hook::server::start_hook_server` closure where `hook_payload` is constructed (around line 473), add at the end of the struct literal:

```rust
                choices: payload.choices.clone(),
                default_choice: payload.default_choice.clone(),
                wezterm_pane_id: payload.wezterm_pane_id.clone(),
                wt_session_id: payload.wt_session_id.clone(),
```

- [ ] **Step 2: Forward fields in `jump_to_terminal` command**

Inside `jump_to_terminal` tauri command where `adapter::HookPayload` is built for the `find_adapter_for_event` call (around line 63), add:

```rust
        choices: None,
        default_choice: None,
        wezterm_pane_id: instance.wezterm_pane_id.clone(),
        wt_session_id: instance.wt_session_id.clone(),
```

And do the same in `respond_permission` (around line 96) and `get_instance_preview` (around line 200).

- [ ] **Step 3: Persist fields from `AdapterState` into `Instance` in the hook handler**

Inside `start_hook_server` closure where `instance` fields are updated from `adapter_state` (around line 538-555), add:

```rust
                    instance.wezterm_pane_id = adapter_state.wezterm_pane_id.clone().or(instance.wezterm_pane_id.clone());
                    instance.wt_session_id = adapter_state.wt_session_id.clone().or(instance.wt_session_id.clone());
```

And in the unmatched/new-instance branch (around line 561), initialize them from `new_state`.

- [ ] **Step 4: Run `cargo check`**

Run: `cd pebble-app/src-tauri && cargo check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add pebble-app/src-tauri/src/main.rs
git commit -m "main: forward wezterm_pane_id and wt_session_id through hook pipeline"
```

---

## Task 4: Persist IDs in ClaudeAdapter::handle_hook

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs`

- [ ] **Step 1: Persist `wezterm_pane_id` and `wt_session_id` from payload into state**

Inside `ClaudeAdapter::handle_hook`, after the `session_name` block (around line 79), add:

```rust
        if let Some(ref pane) = payload.wezterm_pane_id {
            state.wezterm_pane_id = Some(pane.clone());
        }
        if let Some(ref session) = payload.wt_session_id {
            state.wt_session_id = Some(session.clone());
        }
```

- [ ] **Step 2: Run `cargo check`**

Run: `cd pebble-app/src-tauri && cargo check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "adapter(claude): persist wezterm_pane_id and wt_session_id from hooks"
```

---

## Task 5: Capture Env Vars During Discovery (Windows)

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs`
- Modify: `pebble-app/src-tauri/src/adapter/mod.rs`
- Modify: `pebble-app/src-tauri/src/platform/discovery.rs` (optional helper)

- [ ] **Step 1: Add a Windows-specific helper to read process environment variables**

Create a new function in `pebble-app/src-tauri/src/adapter/claude.rs` (or `platform/discovery.rs`):

```rust
#[cfg(target_os = "windows")]
fn read_process_env_var(pid: u32, key: &str) -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
    use windows::Win32::System::Environment::GetEnvironmentStringsW;
    // NOTE: Reading another process's environment block via Win32 is complex.
    // Simpler fallback: if we already have session files or hook events, use those.
    None
}
```

Actually, a simpler and more reliable approach on Windows is to **use `wmic` or `tasklist` is insufficient**, but we can use the existing `sysinfo` crate or WMI to read environment strings. However, `sysinfo` does not expose environment variables of other processes on Windows.

**Recommended simpler approach:** Instead of trying to read other processes' env vars at discovery time, we simply wait for the first hook event from that process (which carries the env vars). Discovery will leave `wezterm_pane_id`/`wt_session_id` as `None`, and the first `UserPromptSubmit` / `PreToolUse` hook will populate them.

So no code changes are needed for discovery — the first hook fills it in.

Skip this task if you agree with the simpler approach.

If you *do* want to implement discovery-time env reading, use the Windows `NtQueryInformationProcess` + `RTL_USER_PROCESS_PARAMETERS` approach. This is complex and out of scope for Plan A.

---

## Task 6: Implement Terminal-Specific Jump on Windows

**Files:**
- Modify: `pebble-app/src-tauri/src/platform/jump.rs`

- [ ] **Step 1: Add `activate_wezterm_pane` helper in the Windows module**

Inside `#[cfg(target_os = "windows")] mod win`, before `jump_to_terminal`, add:

```rust
    fn activate_wezterm_pane(pane_id: &str) -> Result<(), String> {
        let output = std::process::Command::new("wezterm")
            .args(["cli", "activate-tab", "--pane-id", pane_id])
            .output()
            .map_err(|e| format!("Failed to run wezterm cli: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("wezterm cli failed: {}", stderr));
        }
        Ok(())
    }
```

- [ ] **Step 2: Change `jump_to_terminal` signature to accept terminal IDs**

Change from:
```rust
    pub fn jump_to_terminal(pid: u32, _terminal_app: &str) -> Result<(), String> {
```

to:
```rust
    pub fn jump_to_terminal(
        pid: u32,
        terminal_app: &str,
        wezterm_pane_id: Option<&str>,
        wt_session_id: Option<&str>,
    ) -> Result<(), String> {
```

- [ ] **Step 3: Dispatch by terminal_app inside `jump_to_terminal`**

Replace the body of `jump_to_terminal` in the Windows module so it tries terminal-specific activation first, then falls back to window activation:

```rust
    pub fn jump_to_terminal(
        pid: u32,
        terminal_app: &str,
        wezterm_pane_id: Option<&str>,
        _wt_session_id: Option<&str>,
    ) -> Result<(), String> {
        // Terminal-specific precision jump
        if terminal_app == "WezTerm" {
            if let Some(pane) = wezterm_pane_id {
                if let Ok(()) = activate_wezterm_pane(pane) {
                    return Ok(());
                }
            }
        }
        // For WindowsTerminal, wt_session_id could be used here once
        // Microsoft adds a CLI flag to focus by session. For now, fall through.

        // Fallback: window-level activation
        let mut hwnd = find_visible_window(pid);
        if hwnd.0.is_null() {
            let terminal_pid = crate::platform::terminal::detect_terminal_pid(pid);
            if terminal_pid != pid {
                hwnd = find_visible_window(terminal_pid);
            }
        }
        if hwnd.0.is_null() {
            hwnd = find_window_walking_ancestors(pid);
        }

        if hwnd.0.is_null() {
            return Err("Window not found".to_string());
        }

        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
            switch_to_this_window(hwnd, true);
        }
        Ok(())
    }
```

- [ ] **Step 4: Update macOS `jump_to_terminal` signature to match**

```rust
#[cfg(target_os = "macos")]
pub fn jump_to_terminal(
    pid: u32,
    terminal_app: &str,
    _wezterm_pane_id: Option<&str>,
    _wt_session_id: Option<&str>,
) -> Result<(), String> {
    match terminal_app {
        "iTerm2" => {
            if let Some(tty) = get_process_tty(pid) {
                activate_iterm2_session(&tty).map_err(|e| e.to_string())?;
            } else {
                activate_iterm2().map_err(|e| e.to_string())?;
            }
        }
        _ => {}
    }
    Ok(())
}
```

And the non-macOS/non-Windows fallback:
```rust
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn jump_to_terminal(
    _pid: u32,
    _terminal_app: &str,
    _wezterm_pane_id: Option<&str>,
    _wt_session_id: Option<&str>,
) -> Result<(), String> {
    Ok(())
}
```

- [ ] **Step 5: Update `Adapter::jump_to_terminal` trait signature in `adapter/mod.rs`**

Change:
```rust
    fn jump_to_terminal(&self, instance: &Instance) -> Result<(), String>;
```
No change needed — it already takes `&Instance` which now has the new fields. But we must update `ClaudeAdapter::jump_to_terminal` to pass them through:

In `adapter/claude.rs`, update the `jump_to_terminal` impl:
```rust
    fn jump_to_terminal(&self, instance: &Instance) -> Result<(), String> {
        platform::jump::jump_to_terminal(
            instance.pid,
            &instance.terminal_app,
            instance.wezterm_pane_id.as_deref(),
            instance.wt_session_id.as_deref(),
        )
    }
```

- [ ] **Step 6: Run `cargo check`**

Run: `cd pebble-app/src-tauri && cargo check`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add pebble-app/src-tauri/src/platform/jump.rs pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "feat(platform): dispatch jump by terminal app, add WezTerm pane activation"
```

---

## Task 7: Final Integration Test

- [ ] **Step 1: Build the app**

Run: `cd pebble-app/src-tauri && cargo build --bin pebble`
Expected: SUCCESS

- [ ] **Step 2: Run `cargo test`**

Run: `cd pebble-app/src-tauri && cargo test`
Expected: All tests pass

- [ ] **Step 3: Start Pebble in dev mode and verify hooks carry new fields**

Run: `cd pebble-app && npx tauri dev`
Then trigger a `UserPromptSubmit` hook from a WezTerm window and check Pebble logs for `wezterm_pane_id` presence.

- [ ] **Step 4: Test click-to-jump from Pebble UI**

With Pebble running, click an instance that has `terminal_app == "WezTerm"` and a populated `wezterm_pane_id`. It should call `wezterm cli activate-tab --pane-id <id>` and focus the correct tab.

- [ ] **Step 5: Commit final changes if any**

```bash
git add -A
git commit -m "feat: implement Plan A for Windows terminal tab-level jump (WezTerm)"
```

---

## Spec Self-Review

**Spec coverage check:**
- ✅ pebble-bridge captures env vars — Task 2
- ✅ Payload types extended — Task 1
- ✅ Adapter state persists IDs — Task 4
- ✅ main.rs forwards fields — Task 3
- ✅ jump.rs dispatches by terminal_app with WezTerm CLI — Task 6
- ✅ macOS path untouched — verified in Task 6, signature updated but logic unchanged
- ✅ Windows compilation — verified in each task

**Placeholder scan:** No TBDs, TODOs, or vague steps.

**Type consistency:**
- `wezterm_pane_id` and `wt_session_id` are `Option<String>` everywhere.
- `jump_to_terminal` signature is consistent across all `#[cfg]` variants.

**Gap:** Discovery-time env reading is intentionally skipped; first hook event populates the IDs. This is a valid simplification for Plan A.
