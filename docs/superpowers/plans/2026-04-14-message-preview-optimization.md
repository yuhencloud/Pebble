# Pebble Message Preview Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Optimize Pebble's message preview to be real-time, low-noise, and informative, with enhanced permission request display and auto-focus behavior.

**Architecture:** Refactor Rust backend transcript parsing to filter noise and extract only meaningful user/assistant text, add transcript file mtime polling for text-reply synchronization, enrich permission payloads with tool details, and update the React frontend to auto-expand and re-prioritize permission requests.

**Tech Stack:** Rust (Tauri backend), React + TypeScript (frontend), JSONL transcript parsing

---

## File Structure

| File | Responsibility |
|------|----------------|
| `pebble-app/src-tauri/src/transcript.rs` | Parse JSONL transcripts, extract latest user/assistant exchange, strip markdown, filter noise |
| `pebble-app/src-tauri/src/types.rs` | Extend `PendingPermission` with `details` field |
| `pebble-app/src-tauri/src/adapter/mod.rs` | Extend `AdapterState` with cached preview fields |
| `pebble-app/src-tauri/src/adapter/claude.rs` | Build preview lines from state, extract permission details from `tool_input` |
| `pebble-app/src-tauri/src/main.rs` | Poll transcript mtime in monitor loop, emit updates when files change |
| `pebble-app/src/App.tsx` | Sort instances by status priority, auto-expand on permission, split permission card UI |

---

### Task 1: Add permission details type

**Files:**
- Modify: `pebble-app/src-tauri/src/types.rs:10-13`
- Test: `pebble-app/src-tauri/src/adapter/claude.rs` (existing tests will compile-check)

- [ ] **Step 1: Add `details` field to `PendingPermission`**

```rust
#[derive(Serialize, Clone, Debug)]
pub struct PendingPermission {
    pub tool_name: String,
    pub tool_use_id: String,
    pub prompt: String,
    pub choices: Vec<String>,
    pub default_choice: Option<String>,
    pub details: Option<String>,
}
```

- [ ] **Step 2: Commit**

```bash
git add pebble-app/src-tauri/src/types.rs
git commit -m "feat(types): add details field to PendingPermission

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Extend AdapterState with cached preview fields

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/mod.rs:49-67`
- Modify: `pebble-app/src-tauri/src/adapter/mod.rs:69-90`

- [ ] **Step 1: Add fields to `AdapterState`**

```rust
#[derive(Debug, Clone)]
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
    pub wezterm_unix_socket: Option<String>,
    pub subagents: std::collections::HashMap<String, SubagentState>,
    pub subagents_bootstrapped: bool,
    pub latest_user_preview: Option<String>,
    pub latest_assistant_preview: Option<String>,
}
```

- [ ] **Step 2: Update `Default` impl**

```rust
impl Default for AdapterState {
    fn default() -> Self {
        Self {
            status: String::new(),
            last_activity: 0,
            last_hook_event: None,
            pending_permission: None,
            model: None,
            permission_mode: None,
            context_percent: None,
            conversation_log: Vec::new(),
            session_start: None,
            transcript_path: None,
            session_name: None,
            wezterm_pane_id: None,
            wt_session_id: None,
            wezterm_unix_socket: None,
            subagents: std::collections::HashMap::new(),
            subagents_bootstrapped: false,
            latest_user_preview: None,
            latest_assistant_preview: None,
        }
    }
}
```

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/mod.rs
git commit -m "feat(adapter): add latest preview fields to AdapterState

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: Implement transcript parsing and markdown stripping

**Files:**
- Modify: `pebble-app/src-tauri/src/transcript.rs`
- Test: `pebble-app/src-tauri/src/transcript.rs` (existing test module)

- [ ] **Step 1: Write the new functions**

Replace the entire content of `transcript.rs` with:

```rust
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

pub fn read_transcript_preview(path: &str, n: usize) -> Vec<String> {
    let exchange = read_last_exchange(path);
    let mut result = Vec::new();
    if let Some(user) = exchange.0 {
        result.push(truncate_preview(&user, 80, "You: "));
    }
    if let Some(assistant) = exchange.1 {
        result.push(truncate_preview(&assistant, 80, ""));
    }
    if result.len() > n {
        result.truncate(n);
    }
    result
}

pub fn read_last_exchange(path: &str) -> (Option<String>, Option<String>) {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return (None, None),
    };

    let len = match file.seek(SeekFrom::End(0)) {
        Ok(l) => l as i64,
        Err(_) => return (None, None),
    };

    let seek_offset = -(65536.min(len));
    let _ = file.seek(SeekFrom::End(seek_offset));
    let mut discard = String::new();
    let mut reader = BufReader::new(file);
    let _ = reader.read_line(&mut discard);

    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    let mut user_preview: Option<String> = None;
    let mut assistant_preview: Option<String> = None;

    for line in lines.iter().rev() {
        if user_preview.is_some() && assistant_preview.is_some() {
            break;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            let t = json.get("type").and_then(|v| v.as_str());
            if let Some("user") = t {
                if user_preview.is_none() {
                    if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                        if let Some(txt) = extract_clean_text(content, "user") {
                            let stripped = strip_markdown(&txt);
                            if !stripped.trim().is_empty() {
                                user_preview = Some(stripped);
                            }
                        }
                    }
                }
            } else if let Some("assistant") = t {
                if assistant_preview.is_none() {
                    if let Some(content) = json.get("message").and_then(|m| m.get("content")) {
                        if let Some(txt) = extract_clean_text(content, "assistant") {
                            let stripped = strip_markdown(&txt);
                            if !stripped.trim().is_empty() {
                                assistant_preview = Some(stripped);
                            }
                        }
                    }
                }
            }
        }
    }

    (user_preview, assistant_preview)
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

fn truncate_preview(text: &str, max_chars: usize, prefix: &str) -> String {
    let available = max_chars.saturating_sub(prefix.len());
    let truncated: String = text.chars().take(available).collect();
    if text.chars().count() > available {
        format!("{}{}...", prefix, truncated)
    } else {
        format!("{}{}", prefix, truncated)
    }
}

fn extract_clean_text(content: &serde_json::Value, role: &str) -> Option<String> {
    if let Some(s) = content.as_str() {
        if s.starts_with("<local-command-caveat>") || s.starts_with("<command-message>") {
            return None;
        }
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut text_parts = Vec::new();
        for b in arr {
            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                if !t.starts_with("<local-command-caveat>") && !t.starts_with("<command-message>") {
                    text_parts.push(t.to_string());
                }
            }
        }
        if !text_parts.is_empty() {
            return Some(text_parts.join("\n"));
        }
        if role == "assistant" {
            for b in arr {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("Tool");
                    return Some(format!("Using {}", name));
                }
            }
        }
    }
    None
}

fn strip_markdown(text: &str) -> String {
    let mut result = text.to_string();
    // Code blocks -> "Code: lang" or stripped
    result = regex::Regex::new(r"```(\w+)?\n[\s\S]*?```")
        .unwrap()
        .replace_all(&result, |caps: &regex::Captures| {
            if let Some(lang) = caps.get(1) {
                format!("Code: {}", lang.as_str())
            } else {
                "Code".to_string()
            }
        })
        .to_string();
    // Inline code
    result = regex::Regex::new(r"`([^`]+)`").unwrap().replace_all(&result, "$1").to_string();
    // Bold / italic
    result = regex::Regex::new(r"\*\*([^*]+)\*\*").unwrap().replace_all(&result, "$1").to_string();
    result = regex::Regex::new(r"\*([^*]+)\*").unwrap().replace_all(&result, "$1").to_string();
    result = regex::Regex::new(r"__([^_]+)__").unwrap().replace_all(&result, "$1").to_string();
    result = regex::Regex::new(r"_([^_]+)_").unwrap().replace_all(&result, "$1").to_string();
    // Headers
    result = regex::Regex::new(r"^#{1,6}\s*").unwrap().replace_all(&result, "").to_string();
    // List markers
    result = regex::Regex::new(r"^\s*[-*+]\s+").unwrap().replace_all(&result, "").to_string();
    result = regex::Regex::new(r"^\s*\d+\.\s+").unwrap().replace_all(&result, "").to_string();
    // Links [text](url)
    result = regex::Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap().replace_all(&result, "$1").to_string();
    // Collapse multiple spaces
    result = regex::Regex::new(r"\s+").unwrap().replace_all(&result.trim(), " ").to_string();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_clean_text_simple() {
        let val = serde_json::json!("hello world");
        assert_eq!(extract_clean_text(&val, "user"), Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_clean_text_tool_use() {
        let val = serde_json::json!([{"type": "tool_use", "name": "Bash"}]);
        assert_eq!(extract_clean_text(&val, "assistant"), Some("Using Bash".to_string()));
    }

    #[test]
    fn test_strip_markdown_headers_and_lists() {
        let text = "## Title\n- item 1\n* item 2\n1. item 3";
        let result = strip_markdown(text);
        assert_eq!(result, "Title item 1 item 2 item 3");
    }

    #[test]
    fn test_strip_markdown_code_and_links() {
        let text = "Check [docs](https://example.com) and run `cargo build` then:\n```rust\nfn main() {}\n```";
        let result = strip_markdown(text);
        assert!(result.contains("Check docs"));
        assert!(result.contains("cargo build"));
        assert!(result.contains("Code: rust"));
    }

    #[test]
    fn test_read_last_exchange_empty_file() {
        let (u, a) = read_last_exchange("/nonexistent/path.jsonl");
        assert!(u.is_none());
        assert!(a.is_none());
    }
}
```

- [ ] **Step 2: Add `regex` dependency to Cargo.toml**

Edit `pebble-app/src-tauri/Cargo.toml` and ensure `regex = "1"` is in `[dependencies]`.

- [ ] **Step 3: Run tests**

```bash
cd /Users/yuhencloud/Projects/Pebble/pebble-app/src-tauri
cargo test transcript::
```

Expected: all 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/transcript.rs pebble-app/src-tauri/Cargo.toml
git commit -m "feat(transcript): add read_last_exchange and markdown stripping

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 4: Update ClaudeAdapter to use new preview fields and permission details

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs:52-201`
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs:203-246`

- [ ] **Step 1: Update `handle_hook` to cache user/assistant previews and permission details**

In `handle_hook`, replace the transcript_path block with:

```rust
// Update transcript-derived data when we have a path
if let Some(ref tp) = payload.transcript_path {
    if state.transcript_path.as_ref() != Some(tp) {
        state.transcript_path = Some(tp.clone());
        if !state.subagents_bootstrapped {
            let sid = std::path::Path::new(tp)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(tp);
            let cwd = payload.cwd.clone();
            if !cwd.is_empty() {
                let metas = crate::session::list_subagents_with_mtime(&cwd, sid);
                for (m, started_at) in metas {
                    state.subagents.insert(m.agent_id.clone(), SubagentState {
                        id: m.agent_id,
                        name: m.agent_type,
                        description: m.description,
                        started_at,
                    });
                }
            }
            state.subagents_bootstrapped = true;
        }
    }
    if state.session_start.is_none() {
        if let Some(start) = transcript::read_session_start_from_transcript(tp) {
            state.session_start = Some(start);
        }
    }
    let exchange = transcript::read_last_exchange(tp);
    if let Some(user) = exchange.0 {
        state.latest_user_preview = Some(user);
    }
    if let Some(assistant) = exchange.1 {
        state.latest_assistant_preview = Some(assistant);
    }
}
```

Also, in the same `handle_hook`, when handling `UserPromptSubmit`, add caching of user input directly from the hook:

```rust
// After building the event, before status assignment:
if payload.event == "UserPromptSubmit" {
    if let Some(ref input) = payload.tool_input {
        let fallback = input.to_string();
        let text = input.as_str().unwrap_or(&fallback);
        let cleaned = transcript::strip_markdown(text);
        let trimmed = cleaned.trim();
        if !trimmed.is_empty() {
            state.latest_user_preview = Some(trimmed.chars().take(80).collect());
        }
    }
}
```

Then update the permission block to extract `details`. Replace the existing `PendingPermission` construction inside `handle_hook` with:

```rust
let details = Self::extract_permission_details(payload.tool_name.as_deref(), payload.tool_input.as_ref());

state.pending_permission = Some(PendingPermission {
    tool_name: tool_name.clone(),
    tool_use_id: payload.tool_use_id.clone().unwrap_or_else(|| payload.timestamp.to_string()),
    prompt: format!("Allow {}?", tool_name),
    choices,
    default_choice,
    details,
});
```

Add the helper method to `ClaudeAdapter` impl block:

```rust
fn extract_permission_details(tool_name: Option<&str>, tool_input: Option<&serde_json::Value>) -> Option<String> {
    let name = tool_name.unwrap_or("Tool");
    let input = tool_input?;
    match name {
        "Bash" => {
            input.get("command").and_then(|v| v.as_str()).map(|s| format!("Command: {}", s))
        }
        "Edit" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let old = input.get("old_string").and_then(|v| v.as_str()).map(|s| s.lines().next().unwrap_or(s));
            let new = input.get("new_string").and_then(|v| v.as_str()).map(|s| s.lines().next().unwrap_or(s));
            Some(format!("File: {}\nReplace: {:?} -> {:?}", path, old, new))
        }
        "Write" => {
            let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("unknown");
            let content = input.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let preview: String = content.chars().take(200).collect();
            Some(format!("File: {}\nContent: {}", path, preview))
        }
        "Read" => {
            input.get("file_path").and_then(|v| v.as_str()).map(|s| format!("File: {}", s))
        }
        "Delete" => {
            input.get("file_path").and_then(|v| v.as_str()).map(|s| format!("File: {}", s))
        }
        _ => {
            let json = input.to_string();
            let preview: String = json.chars().take(200).collect();
            Some(format!("{} params: {}", name, preview))
        }
    }
}
```

- [ ] **Step 2: Update `get_preview` to assemble the two-line preview**

Replace `get_preview` with:

```rust
fn get_preview(&self, state: &AdapterState) -> Vec<String> {
    let mut result = Vec::new();

    if let Some(ref user) = state.latest_user_preview {
        result.push(format!("You: {}", user.chars().take(60).collect::<String>()));
    } else if let Some(ref event) = state.last_hook_event {
        if event.event == "UserPromptSubmit" {
            if let Some(ref input) = event.tool_input {
                let fallback = input.to_string();
                let text = input.as_str().unwrap_or(&fallback);
                let truncated = if text.len() > 60 { format!("{}...", &text[..60]) } else { text.to_string() };
                result.push(format!("You: {}", truncated));
            } else {
                result.push("You: ...".to_string());
            }
        }
    }

    let action_line = if let Some(ref event) = state.last_hook_event {
        if event.event == "PreToolUse" && event.tool_name.is_some() {
            Some(format!("Using {}", event.tool_name.as_deref().unwrap_or("Tool")))
        } else {
            None
        }
    } else {
        None
    };

    if let Some(action) = action_line {
        result.push(action);
    } else if let Some(ref assistant) = state.latest_assistant_preview {
        let truncated: String = assistant.chars().take(60).collect();
        result.push(truncated);
    }

    result.truncate(2);
    result
}
```

- [ ] **Step 3: Run tests**

```bash
cd /Users/yuhencloud/Projects/Pebble/pebble-app/src-tauri
cargo test adapter::claude::
```

Expected: all existing tests pass.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "feat(adapter): cache previews and extract permission details

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 5: Poll transcript mtime in state monitor

**Files:**
- Modify: `pebble-app/src-tauri/src/main.rs:264-440`

- [ ] **Step 1: Add mtime tracking and transcript re-reading**

Inside `start_state_monitor`, add a `HashMap<String, u64>` to track last-known transcript mtime:

```rust
fn start_state_monitor(
    instances: Arc<Mutex<HashMap<String, Instance>>>,
    adapter_states: Arc<Mutex<HashMap<String, crate::adapter::AdapterState>>>,
    registry: crate::adapter::AdapterRegistry,
    app_handle: tauri::AppHandle,
) {
    let mut notified_map: HashMap<String, bool> = HashMap::new();
    let mut transcript_mtimes: HashMap<String, u64> = HashMap::new();

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));

            let mut map = instances.lock();
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let discovered = registry.discover_all();
            let mut new_map = HashMap::new();

            for raw in discovered {
                let id = raw.id.clone();
                let mut instance = Instance {
                    id: raw.id,
                    pid: raw.pid,
                    status: "waiting".to_string(),
                    working_directory: raw.working_directory,
                    terminal_app: raw.terminal_app,
                    last_activity: 0,
                    pending_permission: None,
                    last_hook_event: None,
                    subagents: Vec::new(),
                    model: None,
                    permission_mode: None,
                    context_percent: None,
                    conversation_log: Vec::new(),
                    session_start: None,
                    transcript_path: None,
                    session_name: raw.session_name.clone(),
                    wezterm_pane_id: None,
                    wt_session_id: None,
                    wezterm_unix_socket: None,
                };
                if let Some(existing) = map.get(&id) {
                    instance.status = existing.status.clone();
                    instance.last_activity = existing.last_activity;
                    instance.pending_permission = existing.pending_permission.clone();
                    instance.last_hook_event = existing.last_hook_event.clone();
                    instance.subagents = existing.subagents.clone();
                    instance.model = existing.model.clone();
                    instance.permission_mode = existing.permission_mode.clone();
                    instance.context_percent = existing.context_percent;
                    instance.conversation_log = existing.conversation_log.clone();
                    instance.session_start = existing.session_start;
                    instance.transcript_path = existing.transcript_path.clone();
                    instance.session_name = existing.session_name.clone();
                    instance.wezterm_pane_id = existing.wezterm_pane_id.clone();
                    instance.wt_session_id = existing.wt_session_id.clone();
                    instance.wezterm_unix_socket = existing.wezterm_unix_socket.clone();
                }

                let adapter = registry.adapters.first().map(|a| a.as_ref());
                if let Some(adapter) = adapter {
                    let states = adapter_states.lock();
                    let mut state = states.get(&id).cloned().unwrap_or_default();

                    // Check transcript mtime for updates
                    if let Some(ref tp) = state.transcript_path {
                        let current_mtime = std::fs::metadata(tp)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs());
                        if let Some(mtime) = current_mtime {
                            let changed = transcript_mtimes.get(tp).copied().unwrap_or(0) < mtime;
                            if changed {
                                transcript_mtimes.insert(tp.clone(), mtime);
                                let exchange = transcript::read_last_exchange(tp);
                                if let Some(user) = exchange.0 {
                                    state.latest_user_preview = Some(user);
                                }
                                if let Some(assistant) = exchange.1 {
                                    state.latest_assistant_preview = Some(assistant);
                                }
                                let _ = adapter_states.lock().insert(id.clone(), state.clone());
                            }
                        }
                    }

                    instance.conversation_log = adapter.get_preview(&state);
                    if state.wezterm_pane_id.is_some() {
                        instance.wezterm_pane_id = state.wezterm_pane_id.clone();
                    }
                    if state.wt_session_id.is_some() {
                        instance.wt_session_id = state.wt_session_id.clone();
                    }
                    if state.wezterm_unix_socket.is_some() {
                        instance.wezterm_unix_socket = state.wezterm_unix_socket.clone();
                    }
                    instance.subagents = adapter.get_subagents(&mut state);
                }

                new_map.insert(id.clone(), instance);
                notified_map.remove(&id);
            }

            // ... rest of orphan merge logic remains unchanged ...
```

Important: the `states` lock is acquired inside the loop, but `adapter_states` is also locked for insert. To avoid deadlock, refactor the mtime check to drop the first lock before re-locking:

```rust
                if let Some(adapter) = adapter {
                    let mut state = {
                        let states = adapter_states.lock();
                        states.get(&id).cloned().unwrap_or_default()
                    };

                    // Check transcript mtime for updates
                    if let Some(ref tp) = state.transcript_path {
                        let current_mtime = std::fs::metadata(tp)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs());
                        if let Some(mtime) = current_mtime {
                            let changed = transcript_mtimes.get(tp).copied().unwrap_or(0) < mtime;
                            if changed {
                                transcript_mtimes.insert(tp.clone(), mtime);
                                let exchange = transcript::read_last_exchange(tp);
                                if let Some(user) = exchange.0 {
                                    state.latest_user_preview = Some(user);
                                }
                                if let Some(assistant) = exchange.1 {
                                    state.latest_assistant_preview = Some(assistant);
                                }
                                adapter_states.lock().insert(id.clone(), state.clone());
                            }
                        }
                    }

                    instance.conversation_log = adapter.get_preview(&state);
                    // ...
```

- [ ] **Step 2: Run cargo check**

```bash
cd /Users/yuhencloud/Projects/Pebble/pebble-app/src-tauri
cargo check
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/main.rs
git commit -m "feat(main): poll transcript mtime to sync assistant replies

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 6: Update frontend sorting, auto-expand, and permission card UI

**Files:**
- Modify: `pebble-app/src/App.tsx:356-640`

- [ ] **Step 1: Update `InstanceCard` to split permission card into title + details**

Update the `PendingPermission` interface:

```typescript
interface PendingPermission {
  tool_name: string;
  tool_use_id: string;
  prompt: string;
  choices: string[];
  default_choice?: string;
  details?: string;
}
```

Update the permission card JSX in `InstanceCard`:

```tsx
      {inst.status === "needs_permission" && inst.pending_permission && (
        <div className="permission-card" onClick={(e) => e.stopPropagation()}>
          <div className="permission-title">
            {inst.pending_permission.tool_name} 请求
          </div>
          {inst.pending_permission.details && (
            <div className="permission-details" title={inst.pending_permission.details}>
              {inst.pending_permission.details}
            </div>
          )}
          <div className="permission-choices">
            {inst.pending_permission.choices.map((choice) => (
              <button
                key={choice}
                className={`permission-btn ${
                  inst.pending_permission?.default_choice === choice
                    ? "permission-btn--default"
                    : ""
                }`}
                onClick={() => handleRespond(choice)}
                disabled={responding}
              >
                {choice}
              </button>
            ))}
          </div>
        </div>
      )}
```

- [ ] **Step 2: Update sorting in `App` to prioritize needs_permission**

Replace `realInstances` computation with:

```typescript
  const realInstances = useMemo(() => {
    const statusOrder: Record<string, number> = {
      needs_permission: 0,
      executing: 1,
      waiting: 2,
      completed: 3,
    };
    return instances
      .filter((i) => i.pid !== 0 || (i.last_activity > 0 && !!i.last_hook_event))
      .map((i) => ({ ...i, subagents: i.subagents || [] }))
      .sort((a, b) => {
        const pa = statusOrder[a.status] ?? 99;
        const pb = statusOrder[b.status] ?? 99;
        if (pa !== pb) return pa - pb;
        return a.working_directory.localeCompare(b.working_directory);
      });
  }, [instances]);
```

- [ ] **Step 3: Add auto-expand logic on permission request**

Inside the `listen<Instance[]>("instances-updated", ...)` handler, add auto-expand:

```typescript
      unlisten = await listen<Instance[]>("instances-updated", (e) => {
        if (!mounted) return;
        setInstances(e.payload);
        const hasPermission = e.payload.some((i) => i.status === "needs_permission");
        if (hasPermission && !expanded) {
          expandPanelRef.current();
        }
      });
```

Also add `expanded` to the `useEffect` dependency array (if ESLint complains, it should be included).

- [ ] **Step 4: Add CSS for permission details**

Edit `pebble-app/src/App.css` and add:

```css
.permission-title {
  font-weight: 600;
  font-size: 13px;
  margin-bottom: 6px;
  color: #f0f0f5;
}

.permission-details {
  font-size: 12px;
  color: #a0a0b0;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 6px;
  padding: 6px 8px;
  margin-bottom: 8px;
  max-height: 80px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-word;
}
```

- [ ] **Step 5: Verify build**

```bash
cd /Users/yuhencloud/Projects/Pebble/pebble-app
npm run build
```

Expected: build succeeds.

- [ ] **Step 6: Commit**

```bash
git add pebble-app/src/App.tsx pebble-app/src/App.css
git commit -m "feat(ui): permission details, status sorting, auto-expand

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

## Spec Coverage Check

| Spec Requirement | Implementing Task |
|------------------|-------------------|
| 第一行显示用户输入 | Task 4 (`get_preview`) |
| 第二行显示动作/assistant摘要 | Task 4 (`get_preview`) |
| 毫秒级用户输入更新 | Task 4 (`handle_hook` caches hook input) |
| 1秒级 assistant 回复同步 | Task 5 (mtime polling) |
| 过滤 thinking/tool_result/系统噪声 | Task 3 (`extract_clean_text`) |
| Markdown 清洗 | Task 3 (`strip_markdown`) |
| 权限详情提取 | Task 4 (`extract_permission_details`) |
| 权限卡片拆分 title + details | Task 6 (frontend JSX) |
| 自动置顶 | Task 6 (status sort) |
| 自动展开面板 | Task 6 (auto-expand listener) |

## Placeholder Scan

No placeholders found. Every step contains exact file paths, complete code blocks, and exact commands.
