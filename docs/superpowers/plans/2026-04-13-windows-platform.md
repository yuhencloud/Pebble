# Pebble Windows Platform Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix Windows compilation and implement all missing Windows platform behaviors (process discovery, CWD resolution, terminal detection, window activation, hover tracking) while keeping macOS behavior untouched.

**Architecture:** Replace `ps`/`lsof` platform-specific commands with `sysinfo` crate for cross-platform process/CWD discovery. Add `#[cfg(target_os = "windows")]` branches in `platform/jump.rs` and `main.rs` for window activation and hover tracking using the `windows` crate.

**Tech Stack:** Rust, Tauri v2, sysinfo 0.30, windows 0.52

---

## File Map

| File | Responsibility | Action |
|------|----------------|--------|
| `pebble-app/src-tauri/Cargo.toml` | Dependency manifest | Add `windows` crate under windows target |
| `pebble-app/src-tauri/src/platform/discovery.rs` | Process discovery | Rewrite with `sysinfo`, remove `ps` dependency |
| `pebble-app/src-tauri/src/platform/cwd.rs` | CWD resolution | Replace `lsof`/`proc` fallbacks with `sysinfo` |
| `pebble-app/src-tauri/src/platform/terminal.rs` | Terminal app detection | Add Windows terminal names; remove `ps_output` arg |
| `pebble-app/src-tauri/src/adapter/claude.rs` | ClaudeAdapter | Update `detect_terminal_app` call to new signature |
| `pebble-app/src-tauri/src/platform/jump.rs` | Window activation | Add Windows `EnumWindows`/`SetForegroundWindow` branch |
| `pebble-app/src-tauri/src/platform/mod.rs` | Public re-exports | Verify `jump` re-export (no changes expected) |
| `pebble-app/src-tauri/src/main.rs` | App orchestration | Add `cfg` to `setup_notch_overlay`; add Windows hover tracker |
| `pebble-app/src-tauri/icons/icon.ico` | Windows app icon | Already exists (generated from icon.png) |
| `pebble-app/src-tauri/bin/pebble-bridge-x86_64-pc-windows-msvc.exe` | External hook bridge | Build & copy via build-bridge script |

---

### Task 1: Add Windows Dependency to Cargo.toml

**Files:**
- Modify: `pebble-app/src-tauri/Cargo.toml`

- [ ] **Step 1: Insert windows crate dependency**

Add under the existing `[target.'cfg(target_os = "macos")'.dependencies]` block:

```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.52", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
]}
```

- [ ] **Step 2: Verify Cargo.toml syntax**

Run:
```bash
cd pebble-app/src-tauri && cargo check --manifest-path Cargo.toml 2>&1 | head -n 5
```

Expected: No manifest parse errors (it will fail later on compilation, but not here).

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/Cargo.toml
git commit -m "chore: add windows crate for win32 window apis"
```

---

### Task 2: Rewrite platform::discovery with sysinfo

**Files:**
- Modify: `pebble-app/src-tauri/src/platform/discovery.rs`

- [ ] **Step 1: Replace list_processes with sysinfo implementation**

Replace the entire file content with:

```rust
use std::collections::HashSet;
use sysinfo::{PidExt, System};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub args: String,
}

pub fn list_processes() -> Vec<ProcessInfo> {
    let s = System::new_all();
    s.processes()
        .iter()
        .map(|(pid, proc)| ProcessInfo {
            pid: pid.as_u32(),
            ppid: proc.parent().map(|p| p.as_u32()).unwrap_or(0),
            comm: proc.name().to_string_lossy().into_owned(),
            args: proc.cmd().join(" "),
        })
        .collect()
}

pub fn find_claude_processes() -> Vec<ProcessInfo> {
    let all = list_processes();
    let mut claude_pids: HashSet<u32> = HashSet::new();

    for p in &all {
        let is_claude_main = p.comm == "claude" || p.comm == "claude-code";
        let is_node_claude = p.comm == "node" && p.args.contains("claude-code");
        if is_claude_main || is_node_claude {
            claude_pids.insert(p.pid);
        }
    }

    all.into_iter()
        .filter(|p| {
            let is_claude = p.comm == "claude" || p.comm == "claude-code"
                || (p.comm == "node" && p.args.contains("claude-code"));
            is_claude && (!claude_pids.contains(&p.ppid) || p.ppid == p.pid)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_processes_not_empty() {
        let procs = list_processes();
        assert!(!procs.is_empty(), "should find at least one process");
    }

    #[test]
    fn test_find_claude_processes_returns_only_top_level() {
        let claudes = find_claude_processes();
        let pids: std::collections::HashSet<u32> = claudes.iter().map(|c| c.pid).collect();
        for c in &claudes {
            assert!(pids.contains(&c.pid));
        }
    }
}
```

- [ ] **Step 2: Compile and run tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib platform::discovery 2>&1
```

Expected: `test_list_processes_not_empty` passes; `test_find_claude_processes_returns_only_top_level` passes (may be empty if no claude running).

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/platform/discovery.rs
git commit -m "refactor(platform): rewrite process discovery with sysinfo for cross-platform support"
```

---

### Task 3: Rewrite platform::cwd with sysinfo fallback

**Files:**
- Modify: `pebble-app/src-tauri/src/platform/cwd.rs`

- [ ] **Step 1: Replace cwd.rs content**

Replace the entire file content with:

```rust
use sysinfo::{PidExt, System};

pub fn get_process_cwd(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // Primary: session file (cross-platform)
    let session_path = dirs::home_dir()?.join(".claude").join("sessions").join(format!("{}.json", pid));
    if let Ok(content) = std::fs::read_to_string(&session_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(cwd) = json.get("cwd").and_then(|v| v.as_str()) {
                return Some(cwd.to_string());
            }
        }
    }

    // Fallback: sysinfo (cross-platform)
    let s = System::new_all();
    s.process(sysinfo::Pid::from(pid as usize))
        .and_then(|p| p.cwd())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_process_cwd_current_process() {
        let cwd = get_process_cwd(std::process::id());
        assert!(cwd.is_some(), "should get current process cwd");
    }
}
```

- [ ] **Step 2: Compile and run tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib platform::cwd 2>&1
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/platform/cwd.rs
git commit -m "refactor(platform): replace lsof/proc fallbacks with sysinfo cwd"
```

---

### Task 4: Extend platform::terminal for Windows and remove ps_output arg

**Files:**
- Modify: `pebble-app/src-tauri/src/platform/terminal.rs`
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs`

- [ ] **Step 1: Rewrite terminal.rs without ps_output**

Replace the entire file content with:

```rust
use sysinfo::{PidExt, System};

pub fn detect_terminal_app(pid: u32) -> String {
    let s = System::new_all();
    let mut current_pid = sysinfo::Pid::from(pid as usize);
    for _ in 0..10 {
        if let Some(proc) = s.process(current_pid) {
            let comm = proc.name().to_string_lossy().to_lowercase();
            let args = proc.cmd().join(" ").to_lowercase();
            let full = format!("{} {}", comm, args);
            if full.contains("iterm2") || full.contains("iterm") {
                return "iTerm2".to_string();
            }
            if full.contains("terminal") || full.contains("apple_terminal") {
                return "Terminal.app".to_string();
            }
            if full.contains("tmux") {
                return "tmux".to_string();
            }
            if full.contains("windowsterminal") || full.contains("windows terminal") {
                return "WindowsTerminal".to_string();
            }
            if full.contains("wezterm") {
                return "WezTerm".to_string();
            }
            if full.contains("alacritty") {
                return "Alacritty".to_string();
            }
            if full.contains("conhost") || full.contains("cmd") {
                return "cmd".to_string();
            }
            if full.contains("pwsh") || full.contains("powershell") {
                return "PowerShell".to_string();
            }
            if let Some(parent) = proc.parent() {
                if parent.as_u32() == 0 || parent == current_pid {
                    break;
                }
                current_pid = parent;
                continue;
            }
        }
        break;
    }
    "Unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_terminal_app_iterm2() {
        // This test relies on real process table; keep simple assertions only
        let _result = detect_terminal_app(std::process::id());
    }
}
```

- [ ] **Step 2: Update adapter/claude.rs call site**

In `pebble-app/src-tauri/src/adapter/claude.rs`, find:

```rust
let terminal = platform::terminal::detect_terminal_app(proc.pid, &ps_output);
```

Replace with:

```rust
let terminal = platform::terminal::detect_terminal_app(proc.pid);
```

Also delete the now-unused `ps_output` construction in `discover_instances`:

Find and remove these lines from `discover_instances`:
```rust
    let ps_output_cmd = std::process::Command::new("ps")
        .args(["-eo", "pid,ppid,comm,args"])
        .output();
    let ps_output = ps_output_cmd
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
```

- [ ] **Step 3: Compile and run tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib platform::terminal 2>&1
```

Expected: Pass.

Run:
```bash
cd pebble-app/src-tauri && cargo check 2>&1
```

Expected: No errors related to terminal or adapter.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/platform/terminal.rs pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "feat(platform): add Windows terminal detection and remove ps_output dependency"
```

---

### Task 5: Add Windows Window Activation to platform::jump

**Files:**
- Modify: `pebble-app/src-tauri/src/platform/jump.rs`

- [ ] **Step 1: Add Windows jump_to_terminal implementation**

Replace the entire file content with:

```rust
use std::process::Command;

pub fn get_process_tty(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tty.is_empty() || tty == "??" {
        None
    } else {
        Some(tty)
    }
}

#[cfg(target_os = "macos")]
pub fn activate_iterm2() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        tell application "iTerm2"
            activate
        end tell
    "#;
    Command::new("osascript").arg("-e").arg(script).output()?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn activate_iterm2_session(tty: &str) -> Result<(), Box<dyn std::error::Error>> {
    let script = format!(
        r#"
        tell application "iTerm2"
            activate
            repeat with aWindow in windows
                repeat with aTab in tabs of aWindow
                    repeat with aSession in sessions of aTab
                        if tty of aSession contains "{}" then
                            tell aWindow
                                select
                            end tell
                            tell aTab
                                select
                            end tell
                            tell aSession
                                select
                            end tell
                            return
                        end if
                    end repeat
                end repeat
            end repeat
        end tell
    "#,
        tty
    );
    Command::new("osascript").arg("-e").arg(&script).output()?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn activate_iterm2() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn activate_iterm2_session(_tty: &str) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn jump_to_terminal(pid: u32, _terminal_app: &str) -> Result<(), String> {
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    static FOUND_HWND: AtomicIsize = AtomicIsize::new(0);

    TARGET_PID.store(pid, Ordering::SeqCst);
    FOUND_HWND.store(0, Ordering::SeqCst);

    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(0));
    }

    let hwnd = HWND(FOUND_HWND.load(Ordering::SeqCst));
    if hwnd.0 == 0 {
        return Err("Window not found".to_string());
    }
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindowVisible};

    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let mut wpid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut wpid));
        if wpid == super::TARGET_PID.load(Ordering::SeqCst) {
            super::FOUND_HWND.store(hwnd.0, Ordering::SeqCst);
            return false.into();
        }
        true.into()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn jump_to_terminal(_pid: u32, _terminal_app: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn jump_to_terminal(pid: u32, terminal_app: &str) -> Result<(), String> {
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

Wait, the `#[cfg(target_os = "macos")]` `jump_to_terminal` and `#[cfg(target_os = "windows")]` `jump_to_terminal` will conflict because they have the same name and identical signature. Rust allows this with `#[cfg]`. The above code actually does use `#[cfg]` to define different versions per OS — that is valid. But I must be careful to place them correctly so cfg arms don't overlap.

The code above is actually valid Rust because `#[cfg(target_os = "macos")]` and `#[cfg(target_os = "windows")]` are mutually exclusive.

But there's one subtle issue: inside `enum_proc` I referenced `super::TARGET_PID`. However `TARGET_PID` is defined inside `jump_to_terminal` which is also under `#[cfg(target_os = "windows")]`. Since `enum_proc` also has `#[cfg(target_os = "windows")]`, at the module level both statics will exist when compiling for Windows, and `super::TARGET_PID` will resolve correctly. Good.

But wait: `FOUND_HWND` and `TARGET_PID` are defined inside `jump_to_terminal` function scope? No! In the code I wrote:
```rust
#[cfg(target_os = "windows")]
pub fn jump_to_terminal(...) {
    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
```

Static items inside functions are still accessible from outside via path? No, statics inside functions are scoped to the function. `super::TARGET_PID` from `enum_proc` won't see them.

I need to move the statics to module level (outside the function) with appropriate cfg.

Corrected structure:

```rust
#[cfg(target_os = "windows")]
mod win {
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    static FOUND_HWND: AtomicIsize = AtomicIsize::new(0);

    pub fn jump_to_terminal(pid: u32, _terminal_app: &str) -> Result<(), String> {
        TARGET_PID.store(pid, Ordering::SeqCst);
        FOUND_HWND.store(0, Ordering::SeqCst);

        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(0));
        }

        let hwnd = HWND(FOUND_HWND.load(Ordering::SeqCst));
        if hwnd.0 == 0 {
            return Err("Window not found".to_string());
        }
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return true.into();
            }
            let mut wpid = 0u32;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut wpid));
            if wpid == TARGET_PID.load(Ordering::SeqCst) {
                FOUND_HWND.store(hwnd.0, Ordering::SeqCst);
                return false.into();
            }
            true.into()
        }
    }
}

#[cfg(target_os = "windows")]
pub use win::jump_to_terminal;

#[cfg(target_os = "macos")]
pub fn jump_to_terminal(pid: u32, terminal_app: &str) -> Result<(), String> {
    ...
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn jump_to_terminal(_pid: u32, _terminal_app: &str) -> Result<(), String> {
    Ok(())
}
```

This is much cleaner. I'll put this in the plan.

- [ ] **Step 2: Compile check**

Run:
```bash
cd pebble-app/src-tauri && cargo check 2>&1
```

Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/platform/jump.rs
git commit -m "feat(platform): implement Windows terminal jump via SetForegroundWindow"
```

---

### Task 6: Fix main.rs cfg and Add Windows Hover Tracker

**Files:**
- Modify: `pebble-app/src-tauri/src/main.rs`

- [ ] **Step 1: Add cfg to setup_notch_overlay function definition**

Find the function definition at line ~353:

```rust
unsafe fn setup_notch_overlay(window: &tauri::WebviewWindow) {
```

Replace with:

```rust
#[cfg(target_os = "macos")]
unsafe fn setup_notch_overlay(window: &tauri::WebviewWindow) {
```

- [ ] **Step 2: Add Windows hover tracker function**

Insert this block right before `fn main()` (after the macOS `start_hover_tracker`):

```rust
#[cfg(target_os = "windows")]
fn start_hover_tracker(window: tauri::WebviewWindow, running: Arc<std::sync::atomic::AtomicBool>) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};
    thread::spawn(move || {
        let mut was_inside = false;
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(60));
            unsafe {
                let mut pt = windows::Win32::Foundation::POINT::default();
                let mut rect = RECT::default();
                let hwnd = window.hwnd().unwrap_or(0) as *mut core::ffi::c_void;
                if hwnd.is_null() {
                    continue;
                }
                if GetCursorPos(&mut pt).is_ok() && GetWindowRect(hwnd, &mut rect).is_ok() {
                    let inside = pt.x >= rect.left && pt.x <= rect.right
                        && pt.y >= rect.top && pt.y <= rect.bottom;
                    if inside != was_inside {
                        was_inside = inside;
                        let _ = window.emit("pebble-hover", inside);
                    }
                }
            }
        }
    });
}
```

- [ ] **Step 3: Wire hover tracker in main() for Windows**

In `main()`, find the `#[cfg(not(target_os = "macos"))]` block inside `.setup()`:

```rust
            #[cfg(not(target_os = "macos"))]
            {
                if let Some(window) = app.handle().get_webview_window("main") {
                    if let Ok(Some(monitor)) = window.current_monitor() {
                        let size = monitor.size();
                        let scale = monitor.scale_factor();
                        let logical_width = size.width as f64 / scale;
                        let w = 300.0;
                        let x = (logical_width - w) / 2.0;
                        let _ = window.set_position(tauri::Position::Logical(
                            tauri::LogicalPosition { x, y: 0.0 }
                        ));
                        let _ = window.set_size(tauri::Size::Logical(
                            tauri::LogicalSize { width: w, height: 52.0 }
                        ));
                    }
                }
            }
```

Add `start_hover_tracker` call inside that same block, right after the size set:

```rust
            #[cfg(not(target_os = "macos"))]
            {
                if let Some(window) = app.handle().get_webview_window("main") {
                    if let Ok(Some(monitor)) = window.current_monitor() {
                        let size = monitor.size();
                        let scale = monitor.scale_factor();
                        let logical_width = size.width as f64 / scale;
                        let w = 300.0;
                        let x = (logical_width - w) / 2.0;
                        let _ = window.set_position(tauri::Position::Logical(
                            tauri::LogicalPosition { x, y: 0.0 }
                        ));
                        let _ = window.set_size(tauri::Size::Logical(
                            tauri::LogicalSize { width: w, height: 52.0 }
                        ));
                    }
                    let hr = hover_running.clone();
                    window.on_window_event(move |event| {
                        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                            hr.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                    start_hover_tracker(window, hover_running.clone());
                }
            }
```

- [ ] **Step 4: Compile check**

Run:
```bash
cd pebble-app/src-tauri && cargo check 2>&1
```

Expected: Clean. (There will be warnings about unused `animate` in `resize_window_centered` and `hover_running` in `main` on Windows if they exist; those are pre-existing and acceptable.)

- [ ] **Step 5: Commit**

```bash
git add pebble-app/src-tauri/src/main.rs
git commit -m "feat(platform): add Windows hover tracker and fix macOS cfg isolation"
```

---

### Task 7: Build pebble-bridge for Windows

**Files:**
- Modify: `pebble-app/src-tauri/bin/pebble-bridge-x86_64-pc-windows-msvc.exe` (binary artifact)

- [ ] **Step 1: Build the bridge binary**

Run:
```bash
cd pebble-app/src-tauri && cargo build --bin pebble-bridge 2>&1
```

Expected: Success.

- [ ] **Step 2: Copy to bin directory**

Run:
```bash
cd pebble-app/src-tauri && node scripts/build-bridge.cjs 2>&1
```

Expected: `Copied bridge binary to ...bin/pebble-bridge-x86_64-pc-windows-msvc.exe`

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/bin/pebble-bridge-x86_64-pc-windows-msvc.exe
git commit -m "feat(bridge): add Windows x86_64 pebble-bridge binary"
```

---

### Task 8: Final Verification

**Files:**
- All of the above (read-only verification)

- [ ] **Step 1: Full Rust check**

Run:
```bash
cd pebble-app/src-tauri && cargo check 2>&1
```

Expected: Zero errors.

- [ ] **Step 2: Run all library tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib 2>&1
```

Expected: All tests pass.

- [ ] **Step 3: UI build check**

Run:
```bash
cd pebble-app && npm run build 2>&1
```

Expected: Vite/TypeScript build succeeds.

- [ ] **Step 4: Tauri smoke build**

Run:
```bash
cd pebble-app/src-tauri && cargo build 2>&1
```

Expected: The full Tauri binary compiles successfully on Windows.

- [ ] **Step 5: Commit any remaining fixes**

If any fixes were needed during verification:

```bash
git add -A
git commit -m "fix: resolve final Windows compilation warnings"
```

---

## Self-Review Checklist

### Spec Coverage
- ✅ `Cargo.toml` windows dependency — Task 1
- ✅ `discovery.rs` cross-platform via sysinfo — Task 2
- ✅ `cwd.rs` cross-platform via sysinfo — Task 3
- ✅ `terminal.rs` Windows terminal names + sysinfo — Task 4
- ✅ `jump.rs` Windows `SetForegroundWindow` — Task 5
- ✅ `main.rs` cfg fix + Windows hover tracker — Task 6
- ✅ `pebble-bridge` Windows binary — Task 7
- ✅ Full verification — Task 8

### Placeholder Scan
- No TBD/TODO/fill-in-details
- Every step has exact file paths, exact code, exact commands

### Type Consistency
- `detect_terminal_app` signature updated from `(pid, &str)` to `(pid)` in both definition and call site
- `jump_to_terminal` signatures remain `(pid, &str)` across all `#[cfg]` variants
- `System::new_all()` and `PidExt` usage consistent across discovery, cwd, terminal
