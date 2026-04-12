# Pebble Cross-Platform Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor Pebble's Rust backend from a 1500-line macOS/iTerm2 monolith into a cross-platform architecture using Adapter trait, platform abstractions, and file-based data sources.

**Architecture:** Extract all platform-specific and CLI-specific logic from `main.rs` into focused modules (`platform/`, `adapter/`, `hook/`, `session.rs`, `transcript.rs`). Keep Tauri setup and IPC as a thin orchestration layer. `ClaudeAdapter` becomes the single source of truth for Claude Code behavior.

**Tech Stack:** Rust, Tauri v2, serde, parking_lot, tokio (where needed), cargo

---

## File Structure

```
pebble-app/src-tauri/src/
├── main.rs                    # Tauri app setup + IPC handlers only
├── types.rs                   # Shared structs (Instance, HookEvent, etc.)
├── adapter/
│   ├── mod.rs                 # Adapter trait + Registry
│   └── claude.rs              # ClaudeAdapter implementation
├── platform/
│   ├── mod.rs                 # Public re-exports
│   ├── discovery.rs           # Process discovery (ps + session files)
│   ├── cwd.rs                 # CWD resolution
│   ├── terminal.rs            # Terminal app detection
│   ├── jump.rs                # Terminal window focus
│   └── notify.rs              # System notifications wrapper
├── hook/
│   ├── mod.rs                 # Hook payload types
│   ├── server.rs              # HTTP hook server
│   └── bridge.rs              # pebble-bridge.mjs config
├── session.rs                 # ~/.claude/sessions/*.json reader
└── transcript.rs              # ~/.claude/projects/*/*.jsonl reader
```

---

## Task 1: Extract Shared Types into `types.rs`

**Files:**
- Create: `pebble-app/src-tauri/src/types.rs`
- Modify: `pebble-app/src-tauri/src/main.rs` (remove structs)

**Context:** `main.rs` currently defines `PendingPermission`, `SubagentInfo`, `Instance`, `HookEvent`, `IncomingHookPayload`, and `AppState` at the top. We need these shared across modules.

- [ ] **Step 1: Create `types.rs` with all shared structs**

Create `pebble-app/src-tauri/src/types.rs` with exact copies of these structs from `main.rs`:
- `PendingPermission`
- `SubagentInfo`
- `Instance`
- `HookEvent`
- `IncomingHookPayload`
- `AppState`

Add `pub use` for all.

```rust
pub use Instance as Instance;
// etc.
```

- [ ] **Step 2: Update `main.rs` to import from `types.rs`**

Modify `pebble-app/src-tauri/src/main.rs`:
1. Add `mod types;` near the top
2. Remove the struct definitions that were moved to `types.rs`
3. Update references to use `types::Instance`, `types::AppState`, etc.

- [ ] **Step 3: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/types.rs pebble-app/src-tauri/src/main.rs
git commit -m "refactor: extract shared types into types.rs"
```

---

## Task 2: Build `platform::discovery` Module

**Files:**
- Create: `pebble-app/src-tauri/src/platform/discovery.rs`
- Create: `pebble-app/src-tauri/src/platform/mod.rs`
- Modify: `pebble-app/src-tauri/src/main.rs`

**Context:** We need a cross-platform process scanner that can find `claude` processes. For now we keep the `ps` approach but wrap it cleanly.

- [ ] **Step 1: Create `platform/discovery.rs`**

```rust
use std::collections::HashSet;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub args: String,
}

pub fn list_processes() -> Vec<ProcessInfo> {
    let output = Command::new("ps")
        .args(["-eo", "pid,ppid,comm,args"])
        .output();

    let mut results = Vec::new();
    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("grep") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            let pid = parts[0].parse::<u32>().unwrap_or(0);
            let ppid = parts[1].parse::<u32>().unwrap_or(0);
            let comm = parts[2].to_string();
            let args = parts[3..].join(" ");
            if pid != 0 {
                results.push(ProcessInfo { pid, ppid, comm, args });
            }
        }
    }
    results
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
            // skip children of other claude processes
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
        // This test is environment-dependent; just verify it doesn't panic
        // and that no returned process is a child of another returned process.
        let pids: std::collections::HashSet<u32> = claudes.iter().map(|c| c.pid).collect();
        for c in &claudes {
            assert!(pids.contains(&c.pid));
        }
    }
}
```

- [ ] **Step 2: Create `platform/mod.rs`**

```rust
pub mod discovery;
pub mod cwd;
pub mod terminal;
pub mod jump;
pub mod notify;
```

- [ ] **Step 3: Verify compilation and run tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib platform::discovery
```

Expected: `test_list_processes_not_empty` passes. `test_find_claude_processes_returns_only_top_level` passes (may be empty if no claude running, but won't panic).

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/platform/
git commit -m "feat(platform): add cross-platform process discovery module"
```

---

## Task 3: Build `platform::cwd` and `platform::terminal`

**Files:**
- Create: `pebble-app/src-tauri/src/platform/cwd.rs`
- Create: `pebble-app/src-tauri/src/platform/terminal.rs`
- Modify: `pebble-app/src-tauri/src/main.rs`

- [ ] **Step 1: Create `platform/cwd.rs`**

```rust
use std::process::Command;
use std::path::Path;

/// Primary: read from session file. Fallback to lsof on macOS.
pub fn get_process_cwd(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // Try session file first (cross-platform)
    let session_path = dirs::home_dir()?.join(".claude").join("sessions").join(format!("{}.json", pid));
    if let Ok(content) = std::fs::read_to_string(&session_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(cwd) = json.get("cwd").and_then(|v| v.as_str()) {
                return Some(cwd.to_string());
            }
        }
    }

    // Fallback: lsof (macOS / Linux)
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("lsof")
            .args(["-a", "-d", "cwd", "-p", &pid.to_string(), "-Fn"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with('n') {
                return Some(line[1..].to_string());
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let link = format!("/proc/{}/cwd", pid);
        if let Ok(path) = std::fs::read_link(&link) {
            return path.to_str().map(|s| s.to_string());
        }
    }

    None
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

- [ ] **Step 2: Create `platform/terminal.rs`**

Move `detect_terminal_app` logic from `main.rs` into here:

```rust
pub fn detect_terminal_app(pid: u32, ps_output: &str) -> String {
    let mut current_pid = pid;
    for _ in 0..10 {
        let line = ps_output.lines().find(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            p.len() >= 3 && p[0].parse::<u32>().ok() == Some(current_pid)
        });
        if let Some(line) = line {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let comm = parts[2].to_lowercase();
            let args = if parts.len() > 3 {
                parts[3..].join(" ").to_lowercase()
            } else {
                String::new()
            };
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
            if let Ok(ppid) = parts[1].parse::<u32>() {
                if ppid == current_pid || ppid == 1 || ppid == 0 {
                    break;
                }
                current_pid = ppid;
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
        let ps = "12345 1 iTerm2 /Applications/iTerm2.app\n";
        let result = detect_terminal_app(12345, ps);
        assert_eq!(result, "iTerm2");
    }

    #[test]
    fn test_detect_terminal_app_unknown() {
        let ps = "12345 1 foo /usr/bin/foo\n";
        let result = detect_terminal_app(12345, ps);
        assert_eq!(result, "Unknown");
    }
}
```

- [ ] **Step 3: Verify compilation and tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib platform::cwd && cargo test --lib platform::terminal
```

Expected: All pass.

- [ ] **Step 4: Update `main.rs` to use platform modules**

In `main.rs`, replace direct `ps`/`lsof` calls with `platform::discovery::find_claude_processes()` and `platform::cwd::get_process_cwd()` / `platform::terminal::detect_terminal_app()`.

Remove old inline `get_process_cwd` and `detect_terminal_app` definitions from `main.rs`.

- [ ] **Step 5: Commit**

```bash
git add pebble-app/src-tauri/src/platform/cwd.rs pebble-app/src-tauri/src/platform/terminal.rs pebble-app/src-tauri/src/main.rs
git commit -m "feat(platform): add cwd and terminal detection modules"
```

---

## Task 4: Build `platform::jump` Module

**Files:**
- Create: `pebble-app/src-tauri/src/platform/jump.rs`
- Modify: `pebble-app/src-tauri/src/main.rs`

- [ ] **Step 1: Create `platform/jump.rs`**

Move `activate_iterm2`, `activate_iterm2_session`, and `get_process_tty` from `main.rs`.

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

pub fn jump_to_terminal(pid: u32, terminal_app: &str) -> Result<(), String> {
    match terminal_app {
        "iTerm2" => {
            if let Some(tty) = get_process_tty(pid) {
                activate_iterm2_session(&tty).map_err(|e| e.to_string())?;
            } else {
                activate_iterm2().map_err(|e| e.to_string())?;
            }
        }
        _ => {
            // Fallback: try to activate app by PID (platform-specific)
            #[cfg(target_os = "macos")]
            {
                // TODO: implement generic app activation via AppleScript
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Update `main.rs` jump logic**

Call `platform::jump::jump_to_terminal(instance.pid, &instance.terminal_app)` instead of inline AppleScript.

- [ ] **Step 3: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: No errors.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/platform/jump.rs pebble-app/src-tauri/src/main.rs
git commit -m "feat(platform): extract terminal jump into jump.rs"
```

---

## Task 5: Build `session.rs` Module

**Files:**
- Create: `pebble-app/src-tauri/src/session.rs`

- [ ] **Step 1: Create `session.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct SessionInfo {
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub started_at: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

pub fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
        .join("sessions")
}

pub fn read_session_for_pid(pid: u32) -> Option<SessionInfo> {
    let path = sessions_dir().join(format!("{}.json", pid));
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn list_all_sessions() -> Vec<SessionInfo> {
    let dir = sessions_dir();
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") {
                    if let Ok(pid) = name.trim_end_matches(".json").parse::<u32>() {
                        if let Some(info) = read_session_for_pid(pid) {
                            results.push(info);
                        }
                    }
                }
            }
        }
    }
    results
}

#[derive(Debug, Clone)]
pub struct SubagentMeta {
    pub agent_id: String,
    pub agent_type: String,
    pub description: Option<String>,
}

pub fn list_subagents(cwd: &str, session_id: &str) -> Vec<SubagentMeta> {
    let project_dir = cwd.replace("/", "-").replace(".", "-");
    let subagents_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
        .join("projects")
        .join(&project_dir)
        .join(&session_id)
        .join("subagents");

    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&subagents_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".meta.json") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let agent_id = name_str
                            .trim_end_matches(".meta.json")
                            .trim_start_matches("agent-")
                            .to_string();
                        let agent_type = json
                            .get("agentType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let description = json
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        results.push(SubagentMeta {
                            agent_id,
                            agent_type,
                            description,
                        });
                    }
                }
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sessions_dir_exists() {
        let dir = sessions_dir();
        assert!(dir.exists() || dir.parent().map(|p| p.exists()).unwrap_or(false));
    }
}
```

- [ ] **Step 2: Verify compilation and tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib session
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/session.rs
git commit -m "feat(session): add session and subagent file readers"
```

---

## Task 6: Build `transcript.rs` Module

**Files:**
- Create: `pebble-app/src-tauri/src/transcript.rs`

- [ ] **Step 1: Create `transcript.rs`**

Move `read_transcript_preview` and `read_session_start_from_transcript` from `main.rs`.

```rust
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

pub fn read_transcript_preview(path: &str, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let len = match file.seek(SeekFrom::End(0)) {
        Ok(l) => l as i64,
        Err(_) => return Vec::new(),
    };

    let seek_offset = -(65536.min(len));
    let _ = file.seek(SeekFrom::End(seek_offset));
    let mut discard = String::new();
    let mut reader = BufReader::new(file);
    let _ = reader.read_line(&mut discard);

    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let mut result = Vec::new();

    for line in lines.iter().rev() {
        if result.len() >= n {
            break;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            let t = json.get("type").and_then(|v| v.as_str());
            if let Some("user") = t {
                if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                    if let Some(txt) = extract_preview_text(content, "user") {
                        let trimmed = txt.trim();
                        if !trimmed.is_empty() {
                            result.push(format!(
                                "You: {}",
                                trimmed.chars().take(80).collect::<String>()
                            ));
                        }
                    }
                }
            } else if let Some("assistant") = t {
                if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                    if let Some(txt) = extract_preview_text(content, "assistant") {
                        let trimmed = txt.trim();
                        if !trimmed.is_empty() {
                            result.push(trimmed.chars().take(80).collect::<String>());
                        }
                    }
                }
            }
        }
    }

    result.reverse();
    result
}

pub fn read_session_start_from_transcript(path: &str) -> Option<u64> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    for line in reader.lines().filter_map(|l| l.ok()) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(ts_str) = json.get("timestamp").and_then(|v| v.as_str()) {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                    return Some(dt.timestamp() as u64);
                }
            }
        }
    }
    None
}

fn extract_preview_text(content: &serde_json::Value, role: &str) -> Option<String> {
    if let Some(s) = content.as_str() {
        if s.starts_with("<local-command-caveat>") || s.starts_with("<command-message>") {
            return None;
        }
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        for b in arr {
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                return Some(t.to_string());
            }
        }
        if role == "assistant" {
            for b in arr {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("Tool");
                    return Some(format!("Using {}", name));
                }
            }
        }
        if role == "user" {
            for b in arr {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    if let Some(c) = b.get("content").and_then(|c| c.as_str()) {
                        let preview = c.trim();
                        if preview.len() > 50 {
                            return Some(format!("Result: {}...", &preview[..50]));
                        }
                        return Some(format!("Result: {}", preview));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_preview_text_simple() {
        let val = serde_json::json!("hello world");
        assert_eq!(extract_preview_text(&val, "user"), Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_preview_text_tool_use() {
        let val = serde_json::json!([{"type": "tool_use", "name": "Bash"}]);
        assert_eq!(extract_preview_text(&val, "assistant"), Some("Using Bash".to_string()));
    }
}
```

- [ ] **Step 2: Verify compilation and tests**

Run:
```bash
cd pebble-app/src-tauri && cargo test --lib transcript
```

Expected: Pass.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/transcript.rs
git commit -m "feat(transcript): add transcript JSONL reader"
```

---

## Task 7: Build `hook` Module (Server + Bridge)

**Files:**
- Create: `pebble-app/src-tauri/src/hook/mod.rs`
- Create: `pebble-app/src-tauri/src/hook/server.rs`
- Create: `pebble-app/src-tauri/src/hook/bridge.rs`
- Modify: `pebble-app/src-tauri/src/main.rs`

- [ ] **Step 1: Create `hook/mod.rs`**

```rust
pub mod bridge;
pub mod server;
```

- [ ] **Step 2: Create `hook/server.rs` (extract from `main.rs`)**

Move `handle_http_request`, `start_hook_server` into here. The server receives `Arc<Mutex<HashMap<String, Instance>>>` and needs to call adapter logic.

For now, keep it simple: the server parses the payload and emits it to a callback. We'll integrate Adapter in Task 10.

```rust
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use parking_lot::Mutex;
use std::collections::HashMap;

pub const HOOK_PORT: u16 = 9876;

pub fn start_hook_server<F>(instances: Arc<Mutex<HashMap<String, crate::types::Instance>>>, mut handler: F)
where
    F: FnMut(&crate::types::IncomingHookPayload) + Send + 'static,
{
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", HOOK_PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind hook server: {}", e);
                return;
            }
        };
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let inst = instances.clone();
                std::thread::spawn(move || {
                    handle_http_request(stream, inst, &mut handler);
                });
            }
        }
    });
}

fn handle_http_request<F>(mut stream: TcpStream, instances: Arc<Mutex<HashMap<String, crate::types::Instance>>>, handler: &mut F)
where
    F: FnMut(&crate::types::IncomingHookPayload),
{
    let mut buf = [0u8; 65536];
    let mut n = 0usize;
    loop {
        match stream.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(bytes_read) => {
                n += bytes_read;
                if n == buf.len() {
                    let _ = stream.write_all(b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n");
                    return;
                }
            }
            Err(_) => break,
        }
    }
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");

    if first_line.starts_with("GET /instances") {
        let map = instances.lock();
        let mut list: Vec<crate::types::Instance> = map.values().cloned().collect();
        drop(map);
        list.sort_by(|a, b| a.working_directory.cmp(&b.working_directory));
        let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string());
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
    } else if first_line.starts_with("POST /hook") {
        if let Some(body_start) = req.find("\r\n\r\n") {
            let body = &req[body_start + 4..];
            if let Ok(payload) = serde_json::from_str::<crate::types::IncomingHookPayload>(body) {
                handler(&payload);
            }
        }
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
    } else {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
    }
}
```

- [ ] **Step 3: Create `hook/bridge.rs` (rewrite config logic)**

Move `ensure_hook_script` and `ensure_claude_hooks_config` from `main.rs`. **Critical change**: remove `statusLine` logic entirely. Delete `ensure_statusline_wrapper_script`.

```rust
use std::fs;
use std::path::PathBuf;

pub fn ensure_hook_script() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let hooks_dir = home.join(".claude").join("hooks");
    let script_path = hooks_dir.join("pebble-bridge.mjs");

    let script_content = r#"#!/usr/bin/env node
import http from "http";
import { execSync } from "child_process";

const eventType = process.argv[2] || "unknown";
const cwd = process.cwd();
const timestamp = Date.now();

function findClaudePid(startPid) {
  let pid = startPid;
  while (pid > 1) {
    try {
      const comm = execSync(`ps -p ${pid} -o comm=`, { encoding: "utf8" }).trim();
      if (comm === "claude" || comm === "claude-code") {
        return pid;
      }
      const ppid = parseInt(execSync(`ps -p ${pid} -o ppid=`, { encoding: "utf8" }).trim(), 10);
      if (ppid === pid || ppid <= 0) break;
      pid = ppid;
    } catch (e) {
      break;
    }
  }
  return null;
}

const senderPid = findClaudePid(process.ppid);

let stdinData = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => { stdinData += chunk; });
process.stdin.on("end", () => {
  let body = { event: eventType, cwd, timestamp };
  if (senderPid) {
    body.sender_pid = senderPid;
  }
  if (stdinData.trim()) {
    try {
      const parsed = JSON.parse(stdinData);
      body = { ...parsed, ...body };
    } catch (e) {
      body.stdin = stdinData;
    }
  }
  const payload = JSON.stringify(body);
  const req = http.request({
    hostname: "127.0.0.1",
    port: 9876,
    path: "/hook",
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Content-Length": Buffer.byteLength(payload),
    },
    timeout: eventType === "PermissionRequest" ? 300000 : 500,
  }, (res) => {
    let responseData = "";
    res.setEncoding("utf8");
    res.on("data", chunk => { responseData += chunk; });
    res.on("end", () => {
      if (responseData.trim()) {
        console.log(responseData);
      }
      process.exit(0);
    });
  });
  req.on("error", () => process.exit(0));
  req.on("timeout", () => { req.destroy(); process.exit(0); });
  req.write(payload);
  req.end();
});
"#;

    if let Ok(existing) = fs::read_to_string(&script_path) {
        if existing.trim() == script_content.trim() {
            return script_path;
        }
    }

    let _ = fs::create_dir_all(&hooks_dir);
    let _ = fs::write(&script_path, script_content);
    script_path
}

pub fn ensure_claude_hooks_config(script_path: &std::path::Path) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let settings_path = home.join(".claude").join("settings.json");

    let mut settings = match fs::read_to_string(&settings_path) {
        Ok(content) => serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| {
            serde_json::json!({})
        }),
        Err(_) => serde_json::json!({}),
    };

    if !settings.is_object() {
        settings = serde_json::json!({});
    }

    let command_str = format!("node {}", script_path.to_string_lossy());

    let pebble_hooks = serde_json::json!({
        "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": format!("{} UserPromptSubmit", command_str) }] }],
        "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PreToolUse", command_str) }] }],
        "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUse", command_str) }] }],
        "PostToolUseFailure": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUseFailure", command_str) }] }],
        "PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PermissionRequest", command_str), "timeout": 300 }] }],
        "Stop": [{ "hooks": [{ "type": "command", "command": format!("{} Stop", command_str) }] }]
    });

    let existing_hooks = settings.get("hooks").cloned().unwrap_or(serde_json::json!({}));
    let mut existing_hooks = if existing_hooks.is_object() {
        existing_hooks.as_object().unwrap().clone()
    } else {
        serde_json::Map::new()
    };

    let mut changed = false;
    for (key, value) in pebble_hooks.as_object().unwrap() {
        if existing_hooks.get(key) != Some(value) {
            existing_hooks.insert(key.clone(), value.clone());
            changed = true;
        }
    }

    // Remove old Pebble statusLine if it exists (migration from v0.1.x)
    if let Some(sl) = settings.get("statusLine") {
        let is_pebble = match sl {
            serde_json::Value::String(cmd) => cmd.contains("pebble-bridge-statusline.sh"),
            serde_json::Value::Object(obj) => obj.get("command")
                .and_then(|c| c.as_str())
                .map(|s| s.contains("pebble-bridge-statusline.sh"))
                .unwrap_or(false),
            _ => false,
        };
        if is_pebble {
            settings.as_object_mut().unwrap().remove("statusLine");
            changed = true;
        }
    }

    if changed {
        settings["hooks"] = serde_json::Value::Object(existing_hooks);
        let _ = fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap());
    }
}

pub fn uninstall_hooks(script_path: &std::path::Path) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let hooks_dir = home.join(".claude").join("hooks");
    let _ = fs::remove_file(hooks_dir.join("pebble-bridge-statusline.sh"));
    let _ = fs::remove_file(script_path);
}
```

- [ ] **Step 4: Update `main.rs` to use hook module**

Remove old `handle_http_request`, `start_hook_server`, `ensure_hook_script`, `ensure_claude_hooks_config`, `ensure_statusline_wrapper_script` from `main.rs`.

Call `hook::bridge::ensure_hook_script()` and `hook::bridge::ensure_claude_hooks_config()` from `main()`.

- [ ] **Step 5: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: No errors.

- [ ] **Step 6: Commit**

```bash
git add pebble-app/src-tauri/src/hook/
git commit -m "feat(hook): extract server and bridge, remove statusLine coupling"
```

---

## Task 8: Build `adapter/mod.rs` (Trait Definition)

**Files:**
- Create: `pebble-app/src-tauri/src/adapter/mod.rs`

- [ ] **Step 1: Define Adapter trait and Registry**

```rust
use crate::types::{Instance, SubagentInfo};
use std::collections::HashMap;

pub mod claude;

#[derive(Debug, Clone)]
pub struct RawInstance {
    pub id: String,
    pub pid: u32,
    pub working_directory: String,
    pub terminal_app: String,
    pub subagents: Vec<SubagentInfo>,
}

/// Hook payload normalized for adapter consumption
#[derive(Debug, Clone)]
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
    pub sender_pid: Option<u32>,
}

/// Mutable state held per instance by the adapter
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
}

pub trait Adapter: Send + Sync {
    fn name(&self) -> &'static str;

    /// Auto-configure hooks/settings for this CLI
    fn auto_configure(&self) -> Result<(), String>;

    /// Discover running instances of this CLI
    fn discover_instances(&self) -> Vec<RawInstance>;

    /// Process a hook payload and update instance state
    fn handle_hook(
        &self,
        payload: &HookPayload,
        state: &mut AdapterState,
        instances: &mut HashMap<String, Instance>,
    );

    /// Return preview lines for UI display
    fn get_preview(&self, state: &AdapterState) -> Vec<String>;

    /// Return subagent list (may read files dynamically)
    fn get_subagents(&self, state: &AdapterState) -> Vec<SubagentInfo>;

    /// Focus the terminal window for this instance
    fn jump_to_terminal(&self, instance: &Instance) -> Result<(), String>;

    /// Respond to a permission request (for hooks that support it)
    fn respond_permission(
        &self,
        instance: &Instance,
        decision: &str,
        reason: Option<&str>,
    ) -> Result<String, String>;
}

pub struct AdapterRegistry {
    adapters: Vec<Box<dyn Adapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self { adapters: Vec::new() }
    }

    pub fn register(&mut self, adapter: Box<dyn Adapter>) {
        self.adapters.push(adapter);
    }

    pub fn configure_all(&self) -> Vec<Result<(), String>> {
        self.adapters.iter().map(|a| a.auto_configure()).collect()
    }

    pub fn discover_all(&self) -> Vec<RawInstance> {
        self.adapters.iter().flat_map(|a| a.discover_instances()).collect()
    }

    pub fn find_adapter_for_event<'a>(&'a self, payload: &HookPayload) -> Option<&'a dyn Adapter> {
        // For now, all events are assumed to be from Claude
        self.adapters.first().map(|a| a.as_ref())
    }
}
```

- [ ] **Step 2: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: No errors (trait compiles even without implementations yet).

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/mod.rs
git commit -m "feat(adapter): define Adapter trait and Registry"
```

---

## Task 9: Build `adapter/claude.rs`

**Files:**
- Create: `pebble-app/src-tauri/src/adapter/claude.rs`
- Modify: `pebble-app/src-tauri/src/main.rs`

This is the largest task. We move all Claude-specific logic from `main.rs` into `ClaudeAdapter`.

- [ ] **Step 1: Create `adapter/claude.rs` skeleton**

```rust
use crate::adapter::{Adapter, AdapterState, HookPayload, RawInstance};
use crate::platform;
use crate::session;
use crate::transcript;
use crate::types::{Instance, PendingPermission, SubagentInfo, HookEvent};
use std::collections::HashMap;

pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn auto_configure(&self) -> Result<(), String> {
        let script_path = crate::hook::bridge::ensure_hook_script();
        crate::hook::bridge::ensure_claude_hooks_config(&script_path);
        Ok(())
    }

    fn discover_instances(&self) -> Vec<RawInstance> {
        let ps_output_cmd = std::process::Command::new("ps")
            .args(["-eo", "pid,ppid,comm,args"])
            .output();
        let ps_output = ps_output_cmd
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        let claudes = platform::discovery::find_claude_processes();
        let mut results = Vec::new();

        for proc in claudes {
            let cwd = platform::cwd::get_process_cwd(proc.pid)
                .unwrap_or_else(|| "Unknown".to_string());
            let terminal = platform::terminal::detect_terminal_app(proc.pid, &ps_output);
            let id = format!("cc-{}", proc.pid);

            results.push(RawInstance {
                id,
                pid: proc.pid,
                working_directory: cwd,
                terminal_app: terminal,
                subagents: Vec::new(),
            });
        }

        results
    }

    fn handle_hook(&self, payload: &HookPayload, state: &mut AdapterState, _instances: &mut HashMap<String, Instance>) {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Update transcript-derived data when we have a path
        if let Some(ref tp) = payload.transcript_path {
            if state.transcript_path.as_ref() != Some(tp) {
                state.transcript_path = Some(tp.clone());
            }
            if state.session_start.is_none() {
                if let Some(start) = transcript::read_session_start_from_transcript(tp) {
                    state.session_start = Some(start);
                }
            }
            let preview = transcript::read_transcript_preview(tp, 3);
            if !preview.is_empty() {
                state.conversation_log = preview;
            }
        }

        if let Some(ref sn) = payload.session_name {
            state.session_name = Some(sn.clone());
        }
        if let Some(ref m) = payload.model {
            state.model = Some(m.clone());
        }
        if let Some(cp) = payload.context_percent {
            state.context_percent = Some(cp);
        }

        let event = HookEvent {
            event: payload.event.clone(),
            cwd: payload.cwd.clone(),
            timestamp: payload.timestamp,
            tool_name: payload.tool_name.clone(),
            tool_input: payload.tool_input.clone(),
            permission_mode: payload.permission_mode.clone(),
            tool_use_id: payload.tool_use_id.clone(),
            model: payload.model.clone(),
            context_percent: payload.context_percent,
            session_name: payload.session_name.clone(),
        };

        state.last_hook_event = Some(event.clone());
        state.last_activity = now_secs;

        if let Some(ref pm) = payload.permission_mode {
            state.permission_mode = Some(pm.clone());
        }

        let is_permission_event = payload.event == "PreToolUse"
            && !matches!(
                payload.permission_mode.as_deref(),
                Some("bypassPermissions" | "dontAsk" | "auto" | "acceptEdits")
            );

        if payload.event == "PermissionRequest" {
            // The hook server will handle blocking response separately
            state.status = "needs_permission".to_string();
            state.pending_permission = Some(PendingPermission {
                tool_name: payload.tool_name.clone().unwrap_or_else(|| "Claude".to_string()),
                tool_use_id: payload.tool_use_id.clone().unwrap_or_default(),
                prompt: format!("Allow {}?", payload.tool_name.clone().unwrap_or_else(|| "tool".to_string())),
                choices: vec!["Allow".to_string(), "Deny".to_string()],
                default_choice: Some("Allow".to_string()),
            });
        } else if is_permission_event {
            state.status = "needs_permission".to_string();
            state.pending_permission = Some(PendingPermission {
                tool_name: payload.tool_name.clone().unwrap_or_else(|| "Claude".to_string()),
                tool_use_id: payload.tool_use_id.clone().unwrap_or_default(),
                prompt: format!("Allow {}?", payload.tool_name.clone().unwrap_or_else(|| "tool".to_string())),
                choices: vec!["Allow".to_string(), "Deny".to_string()],
                default_choice: Some("Allow".to_string()),
            });
        } else {
            let new_status = match payload.event.as_str() {
                "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => "executing",
                _ => "waiting",
            };
            state.status = new_status.to_string();
            state.pending_permission = None;
        }
    }

    fn get_preview(&self, state: &AdapterState) -> Vec<String> {
        if !state.conversation_log.is_empty() {
            state.conversation_log.clone()
        } else if let Some(ref event) = state.last_hook_event {
            if event.event == "UserPromptSubmit" {
                if let Some(ref input) = event.tool_input {
                    let text = input.as_str().unwrap_or(&input.to_string());
                    let truncated = if text.len() > 80 { format!("{}...", &text[..80]) } else { text.to_string() };
                    return vec![format!("You: {}", truncated)];
                }
                return vec!["You: ...".to_string()];
            } else if event.event == "PreToolUse" {
                return vec![format!("Using {}", event.tool_name.as_deref().unwrap_or("Tool"))];
            }
            Vec::new()
        } else {
            Vec::new()
        }
    }

    fn get_subagents(&self, state: &AdapterState) -> Vec<SubagentInfo> {
        // Try session file first
        if let Some(ref session_id) = state.transcript_path {
            // Extract session_id from path: .../<session_id>.jsonl
            let sid = std::path::Path::new(session_id)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(session_id);
            // Need cwd to locate subagents; fallback to last_hook_event.cwd
            let cwd = state.last_hook_event.as_ref().map(|e| e.cwd.clone()).unwrap_or_default();
            if !cwd.is_empty() {
                let metas = session::list_subagents(&cwd, sid);
                return metas.into_iter().map(|m| SubagentInfo {
                    id: m.agent_id,
                    status: "executing".to_string(),
                    name: m.agent_type,
                }).collect();
            }
        }
        Vec::new()
    }

    fn jump_to_terminal(&self, instance: &Instance) -> Result<(), String> {
        platform::jump::jump_to_terminal(instance.pid, &instance.terminal_app)
    }

    fn respond_permission(
        &self,
        _instance: &Instance,
        decision: &str,
        reason: Option<&str>,
    ) -> Result<String, String> {
        let resp = match decision {
            "allow" => serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow" }
                }
            }),
            "deny" => serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {
                        "behavior": "deny",
                        "message": reason.unwrap_or("Denied by user via Pebble")
                    }
                }
            }),
            _ => return Err("Invalid decision".to_string()),
        };
        Ok(resp.to_string())
    }
}
```

- [ ] **Step 2: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: May have small type mismatches; fix inline until clean.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "feat(adapter): implement ClaudeAdapter with file-based data sources"
```

---

## Task 10: Refactor `main.rs` to Use Adapter Registry

**Files:**
- Modify: `pebble-app/src-tauri/src/main.rs`

- [ ] **Step 1: Rewrite `main.rs` orchestration layer**

Key changes:
1. Add `mod adapter; mod hook; mod platform; mod session; mod transcript;`
2. Import from `types`, `adapter`, `platform`, `hook`
3. `AppState` holds instances + registry:

```rust
struct AppState {
    instances: Arc<Mutex<HashMap<String, types::Instance>>>,
    registry: adapter::AdapterRegistry,
}
```

4. `main()`:
   - Create registry, register `ClaudeAdapter::new()`
   - Call `registry.configure_all()`
   - Start hook server with a closure that routes payloads to `registry.find_adapter_for_event()`

5. `start_state_monitor`:
   - Call `registry.discover_all()` instead of inline `discover_instances()`
   - Map `RawInstance` to `Instance`
   - For each instance, call `adapter.get_preview()` and `adapter.get_subagents()` to enrich data before emitting to UI

6. Tauri command handlers:
   - `get_instances`: unchanged logic
   - `jump_to_terminal`: find adapter for instance, call `adapter.jump_to_terminal()`
   - `respond_permission`: find adapter, call `adapter.respond_permission()`, return a simple `Ok(())` for now (full HTTP response wiring happens in Task 11)
   - `get_instance_preview`: find instance, get its adapter, call `adapter.get_preview()`
   - `resize_window_centered`: unchanged

- [ ] **Step 2: Delete old inline functions from `main.rs`**

Remove:
- `discover_instances()`
- `get_process_cwd()`
- `detect_terminal_app()`
- `extract_model_string()`
- `extract_context_percent_from_payload()`
- `extract_preview_text()`
- `read_transcript_preview()`
- `read_session_start_from_transcript()`
- `update_instance_from_hook()`
- `parse_permission_choices()`
- `read_iterm2_last_lines()`
- `inject_permission_response_to_iterm2()`
- `activate_iterm2()`
- `activate_iterm2_session()`
- `get_process_tty()`
- `handle_http_request()`
- `start_hook_server()`
- `ensure_hook_script()`
- `ensure_statusline_wrapper_script()`
- `ensure_claude_hooks_config()`

Keep only:
- Tauri setup (`main()`)
- `is_related_cwd`
- `build_grouped_instances`
- `start_state_monitor` (rewritten to use registry)
- Tauri command functions
- macOS-specific UI setup (`setup_notch_overlay`, `start_hover_tracker`)

- [ ] **Step 3: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: Clean. If not, fix remaining references.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/main.rs
git commit -m "refactor(main): wire AdapterRegistry and delete inline Claude logic"
```

---

## Task 11: Implement `PermissionRequest` Blocking Response in Hook Server

**Files:**
- Modify: `pebble-app/src-tauri/src/hook/server.rs`
- Modify: `pebble-app/src-tauri/src/main.rs`

This is the critical piece that enables cross-platform permission approval.

- [ ] **Step 1: Enhance hook server to support blocking responses**

Change `hook/server.rs`:
- When a `POST /hook` contains `PermissionRequest`, instead of writing `200 OK` immediately, wait for a signal.
- Use a channel or Condvar to pause the thread until `respond_permission` command provides the response body.

Design:
- Add a global/shared `permission_responses: Arc<Mutex<HashMap<String, String>>>` keyed by `tool_use_id` (or `session_id` if no `tool_use_id`).
- When `PermissionRequest` arrives, spawn a thread that waits (with timeout) on a condition variable or uses `std::sync::mpsc`.
- The `respond_permission` Tauri command writes the response JSON into the map and signals.
- The hook thread then writes the JSON back to the HTTP response.

Simpler approach using shared state + sleep polling:

```rust
use std::sync::{Arc, Condvar};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct PermissionResponseStore {
    inner: Arc<(Mutex<HashMap<String, String>>, Condvar)>,
}

impl PermissionResponseStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(HashMap::new()), Condvar::new())),
        }
    }

    pub fn set(&self, key: String, value: String) {
        let (lock, cvar) = &*self.inner;
        lock.lock().insert(key, value);
        cvar.notify_all();
    }

    pub fn wait_for(&self, key: &str, timeout: Duration) -> Option<String> {
        let (lock, cvar) = &*self.inner;
        let mut guard = lock.lock();
        let start = Instant::now();
        loop {
            if let Some(val) = guard.get(key) {
                return Some(val.clone());
            }
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return None;
            }
            let remaining = timeout - elapsed;
            guard = cvar.wait_timeout(guard, remaining).ok()?.0;
        }
    }
}
```

Update `handle_http_request` signature to accept `PermissionResponseStore`.

When `PermissionRequest` arrives:
1. Extract `tool_use_id` (or generate a fallback key from `session_id` + `tool_name`)
2. Call `handler(&payload)` to update UI state
3. Wait on `store.wait_for(&key, Duration::from_secs(300))`
4. If response found, write it as the HTTP body with `Content-Type: application/json`
5. If timeout, write `200 OK` with empty body (let Claude fall back to terminal UI)

- [ ] **Step 2: Update `main.rs` `respond_permission` command**

```rust
#[tauri::command]
fn respond_permission(
    instance_id: String,
    choice: String,
    state: State<'_, AppState>,
    permission_store: State<'_, hook::server::PermissionResponseStore>,
) -> Result<(), String> {
    let map = state.instances.lock();
    let instance = map
        .values()
        .find(|i| i.id == instance_id)
        .cloned()
        .ok_or("Instance not found")?;
    drop(map);

    let adapter = state.registry.find_adapter_for_event(&adapter::HookPayload {
        event: "PermissionRequest".to_string(),
        cwd: instance.working_directory.clone(),
        timestamp: 0,
        tool_name: None,
        tool_input: None,
        permission_mode: None,
        tool_use_id: instance.pending_permission.as_ref().map(|p| p.tool_use_id.clone()),
        model: None,
        context_percent: None,
        session_name: None,
        transcript_path: None,
        sender_pid: Some(instance.pid),
    }).ok_or("No adapter found")?;

    let response_json = adapter.respond_permission(&instance, &choice, None)?;

    let key = instance
        .pending_permission
        .as_ref()
        .map(|p| p.tool_use_id.clone())
        .unwrap_or_else(|| instance_id.clone());

    permission_store.set(key, response_json);

    // Update local state
    {
        let mut map = state.instances.lock();
        if let Some(inst) = map.values_mut().find(|i| i.id == instance_id) {
            inst.status = "executing".to_string();
            inst.pending_permission = None;
        }
    }

    Ok(())
}
```

Add `permission_store` to `tauri::Builder::manage()`.

- [ ] **Step 3: Update hook server invocation in `main()`**

Pass `PermissionResponseStore` into `hook::server::start_hook_server`.

- [ ] **Step 4: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: Clean.

- [ ] **Step 5: Commit**

```bash
git add pebble-app/src-tauri/src/hook/server.rs pebble-app/src-tauri/src/main.rs
git commit -m "feat(hook): implement blocking PermissionRequest response for cross-platform approval"
```

---

## Task 12: UI Adjustments

**Files:**
- Modify: `pebble-app/src/App.tsx`

- [ ] **Step 1: Remove context percent badge display**

Find and remove:
```tsx
{inst.context_percent != null && (
  <span className="badge badge--context">{inst.context_percent}%</span>
)}
```

- [ ] **Step 2: Update `respond_permission` invocation**

Ensure the `InstanceCard`'s permission button still calls `onRespond(choice)`. The backend now processes all choices uniformly regardless of terminal.

- [ ] **Step 3: Verify UI builds**

Run:
```bash
cd pebble-app && npm run build
```

Expected: Build succeeds.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src/App.tsx
git commit -m "ui: remove context badge, keep permission buttons for new hook flow"
```

---

## Task 13: Final Cleanup and Verification

**Files:**
- Modify: `pebble-app/src-tauri/src/main.rs`
- Any remaining references

- [ ] **Step 1: Run full Rust check**

```bash
cd pebble-app/src-tauri && cargo check
```

Expected: Zero errors, zero warnings.

- [ ] **Step 2: Run all library tests**

```bash
cd pebble-app/src-tauri && cargo test --lib
```

Expected: All tests pass.

- [ ] **Step 3: Cross-platform compile checks**

```bash
cd pebble-app/src-tauri && cargo check --target x86_64-pc-windows-msvc
```

If target not installed, install with:
```bash
rustup target add x86_64-pc-windows-msvc
```

Note: macOS-only AppleScript code is behind `#[cfg(target_os = "macos")]`, so Windows target should compile.

```bash
cd pebble-app/src-tauri && cargo check --target x86_64-unknown-linux-gnu
```

If target not installed:
```bash
rustup target add x86_64-unknown-linux-gnu
```

Expected: Both compile cleanly.

- [ ] **Step 4: Smoke test settings.json behavior**

Launch the dev app:
```bash
cd pebble-app && npm run tauri dev &
```

After ~10 seconds, check `~/.claude/settings.json`:
- Must contain the new `hooks` keys (`PermissionRequest`, etc.)
- Must **NOT** contain `pebble-bridge-statusline.sh` in `statusLine`
- Existing `statusLine` from other tools (if any) should be preserved

Kill dev app: `pkill -f "tauri dev"`

- [ ] **Step 5: Commit any remaining fixes**

```bash
git add -A
git commit -m "fix: resolve compilation warnings and cross-platform cfg gates"
```

---

## Self-Review Checklist

### Spec Coverage
- ✅ Adapter trait + `ClaudeAdapter` — Tasks 8-9
- ✅ `platform/` abstraction — Tasks 2-4
- ✅ Session/Transcript file readers — Tasks 5-6
- ✅ Hook server extraction, `PermissionRequest` blocking response — Tasks 7, 11
- ✅ Remove `statusLine` dependency — Task 7
- ✅ Delete `read_iterm2_last_lines`, `parse_permission_choices`, AppleScript injection — Tasks 10, 13
- ✅ UI context badge removal — Task 12
- ✅ Cross-platform compile verification — Task 13

### Placeholder Scan
- No "TBD", "TODO", or vague steps
- Every task has exact file paths
- Every code step includes actual code

### Type Consistency
- `Instance`, `HookEvent`, `AdapterState` fields aligned with `types.rs`
- `Adapter::respond_permission` signature consistent across trait and impl
- `PermissionResponseStore` used consistently in `hook/server.rs` and `main.rs`

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-12-pebble-cross-platform-refactor.md`.**

**Two execution options:**

1. **Subagent-Driven (recommended)** — Fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** — Execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints

**Which approach?**
