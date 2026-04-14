# Subagent Lifecycle Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add precise SubagentStart/SubagentStop hook tracking so subagents appear when they start, disappear when they stop, and are cleaned up after a 600s timeout, with a collapsible UI.

**Architecture:** Extend `IncomingHookPayload` and `AdapterState` to carry subagent metadata. Use hook events to mutate an in-memory `HashMap<String, SubagentState>` per instance, falling back to filesystem scan with `.jsonl` mtime when Pebble starts after a subagent. The frontend renders a single collapsed summary line that expands to show the full list.

**Tech Stack:** Rust (Tauri backend), React + TypeScript (frontend)

---

## File Structure

| File | Responsibility |
|------|----------------|
| `pebble-app/src-tauri/src/types.rs` | Add `agent_id`/`agent_type` to `IncomingHookPayload` |
| `pebble-app/src-tauri/src/adapter/mod.rs` | Add `SubagentState` struct and `subagents` map to `AdapterState` |
| `pebble-app/src-tauri/src/session.rs` | Add `mtime` fallback helper for subagent discovery |
| `pebble-app/src-tauri/src/adapter/claude.rs` | Handle `SubagentStart`/`SubagentStop`; implement timeout + fallback in `get_subagents` |
| `pebble-app/src-tauri/src/hook/bridge.rs` | Auto-configure `SubagentStart`/`SubagentStop` hooks (append-only) |
| `pebble-app/src-tauri/src/main.rs` | Forward `agent_id`/`agent_type` from raw hook payload to adapter |
| `pebble-app/src/App.tsx` | Collapsible subagent list + tooltips |

---

### Task 1: Extend Payload and Adapter Types

**Files:**
- Modify: `pebble-app/src-tauri/src/types.rs`
- Modify: `pebble-app/src-tauri/src/adapter/mod.rs`
- Test: `pebble-app/src-tauri/src/adapter/mod.rs` (via existing test module)

**Context:** The hook payload and adapter state need to know about subagent identity before we can process lifecycle events.

- [ ] **Step 1: Add `agent_id` and `agent_type` to `IncomingHookPayload`**

In `pebble-app/src-tauri/src/types.rs`, add two fields inside `IncomingHookPayload`:

```rust
#[serde(default, rename = "agent_id")]
pub agent_id: Option<String>,
#[serde(default, rename = "agent_type")]
pub agent_type: Option<String>,
```

Place them after `wezterm_unix_socket` (around line 113).

- [ ] **Step 2: Add `SubagentState` and update `AdapterState`**

In `pebble-app/src-tauri/src/adapter/mod.rs`, after the `HookPayload` struct, add:

```rust
#[derive(Debug, Clone)]
pub struct SubagentState {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub started_at: u64,
}
```

Then add to `AdapterState`:

```rust
pub subagents: std::collections::HashMap<String, SubagentState>,
```

Update the `Default` impl for `AdapterState`. Since it currently derives `Default`, replace the derive with a manual impl:

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
        }
    }
}
```

Remove `#[derive(Default)]` from `AdapterState`.

- [ ] **Step 3: Verify compilation**

Run:
```bash
cd pebble-app/src-tauri && cargo check
```

Expected: clean compile with no errors.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/types.rs pebble-app/src-tauri/src/adapter/mod.rs
git commit -m "feat(types): add agent_id/agent_type and SubagentState"
```

---

### Task 2: Add Subagent Discovery with mtime Fallback

**Files:**
- Modify: `pebble-app/src-tauri/src/session.rs`
- Test: `pebble-app/src-tauri/src/session.rs`

**Context:** When Pebble starts after a subagent, we scan files. We need the `.jsonl` mtime to set a realistic `started_at`.

- [ ] **Step 1: Write a new public function `list_subagents_with_mtime`**

In `pebble-app/src-tauri/src/session.rs`, add this function after `list_subagents`:

```rust
pub fn list_subagents_with_mtime(cwd: &str, session_id: &str) -> Vec<(SubagentMeta, u64)> {
    let project_dir = cwd.replace("\\", "-")
        .replace("/", "-")
        .replace(":", "-")
        .replace(".", "-");
    let subagents_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
        .join("projects")
        .join(&project_dir)
        .join(&session_id)
        .join("subagents");

    let mut results = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

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

                        let jsonl_path = entry.path().with_file_name(
                            format!("agent-{}.jsonl", agent_id)
                        );
                        let started_at = std::fs::metadata(&jsonl_path)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .or_else(|| {
                                std::fs::metadata(entry.path())
                                    .and_then(|m| m.modified())
                                    .ok()
                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs())
                            })
                            .unwrap_or(now);

                        results.push((
                            SubagentMeta {
                                agent_id,
                                agent_type,
                                description,
                            },
                            started_at,
                        ));
                    }
                }
            }
        }
    }
    results
}
```

- [ ] **Step 2: Write a minimal test**

Add a test in the `#[cfg(test)]` block at the bottom of `session.rs`:

```rust
#[test]
fn test_list_subagents_with_mtime_returns_empty_for_missing_dir() {
    let results = list_subagents_with_mtime("/definitely/not/a/real/path", "fake-session");
    assert!(results.is_empty());
}
```

- [ ] **Step 3: Run tests**

```bash
cd pebble-app/src-tauri && cargo test --lib session::tests -- --nocapture
```

Expected: `test_list_subagents_with_mtime_returns_empty_for_missing_dir ... ok`

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/session.rs
git commit -m "feat(session): add list_subagents_with_mtime helper"
```

---

### Task 3: Handle SubagentStart/SubagentStop and Timeout

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs`
- Test: `pebble-app/src-tauri/src/adapter/claude.rs` (add a test module at the bottom)

**Context:** The adapter is the owner of subagent state. It reacts to hooks and exposes the current list via `get_subagents`.

- [ ] **Step 1: Import `SubagentState`**

At the top of `pebble-app/src-tauri/src/adapter/claude.rs`, change the import to:

```rust
use crate::adapter::{Adapter, AdapterState, HookPayload, RawInstance, SubagentState};
```

- [ ] **Step 2: Handle `SubagentStart` and `SubagentStop` in `handle_hook`**

Inside `fn handle_hook`, before the `let event = ...` line, add:

```rust
match payload.event.as_str() {
    "SubagentStart" => {
        if let (Some(id), Some(agent_type)) = (payload.agent_id.as_ref(), payload.agent_type.as_ref()) {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            state.subagents.insert(id.clone(), SubagentState {
                id: id.clone(),
                name: agent_type.clone(),
                description: None,
                started_at: now_secs,
            });
        }
    }
    "SubagentStop" => {
        if let Some(id) = payload.agent_id.as_ref() {
            state.subagents.remove(id);
        }
    }
    _ => {}
}
```

- [ ] **Step 3: Update `get_subagents` with timeout cleanup and file fallback**

Replace the existing `get_subagents` implementation with:

```rust
const SUBAGENT_TIMEOUT_SECS: u64 = 600;

fn get_subagents(&self, state: &AdapterState) -> Vec<SubagentInfo> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 1. Timeout cleanup
    let mut cleaned = state.subagents.clone();
    cleaned.retain(|_, s| now_secs.saturating_sub(s.started_at) <= SUBAGENT_TIMEOUT_SECS);

    // 2. File fallback for Pebble restarts
    if let Some(ref session_id) = state.transcript_path {
        let sid = std::path::Path::new(session_id)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(session_id);
        let cwd = state.last_hook_event.as_ref().map(|e| e.cwd.clone()).unwrap_or_default();
        if !cwd.is_empty() {
            let metas = crate::session::list_subagents_with_mtime(&cwd, sid);
            for (m, started_at) in metas {
                if !cleaned.contains_key(&m.agent_id) {
                    cleaned.insert(m.agent_id.clone(), SubagentState {
                        id: m.agent_id,
                        name: m.agent_type,
                        description: m.description,
                        started_at,
                    });
                }
            }
        }
    }

    cleaned.into_values().map(|s| {
        let full_name = if let Some(ref d) = s.description {
            format!("{} {}", s.name, d)
        } else {
            s.name.clone()
        };
        SubagentInfo {
            id: s.id,
            status: "executing".to_string(),
            name: full_name,
        }
    }).collect()
}
```

Note: because `get_subagents` takes `&AdapterState` (immutable), we clone the map here. The clone is cheap because the map is tiny.

- [ ] **Step 4: Write a unit test for subagent lifecycle**

At the bottom of `claude.rs`, add a test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_start_stop_lifecycle() {
        let adapter = ClaudeAdapter::new();
        let mut state = AdapterState::default();

        let payload = HookPayload {
            event: "SubagentStart".to_string(),
            cwd: "/tmp".to_string(),
            timestamp: 0,
            tool_name: None,
            tool_input: None,
            permission_mode: None,
            tool_use_id: None,
            model: None,
            context_percent: None,
            session_name: None,
            transcript_path: None,
            choices: None,
            default_choice: None,
            wezterm_pane_id: None,
            wt_session_id: None,
            wezterm_unix_socket: None,
            agent_id: Some("agent-123".to_string()),
            agent_type: Some("Explore".to_string()),
        };

        adapter.handle_hook(&payload, &mut state, &mut std::collections::HashMap::new());
        assert_eq!(state.subagents.len(), 1);
        assert!(state.subagents.contains_key("agent-123"));

        let stop_payload = HookPayload {
            event: "SubagentStop".to_string(),
            agent_id: Some("agent-123".to_string()),
            ..payload
        };
        adapter.handle_hook(&stop_payload, &mut state, &mut std::collections::HashMap::new());
        assert!(state.subagents.is_empty());
    }
}
```

Note: this requires `agent_id` and `agent_type` to exist on `HookPayload` as well (see Task 1). Make sure `HookPayload` in `adapter/mod.rs` also has these fields.

Wait — Task 1 only added them to `IncomingHookPayload`. We must also add them to `HookPayload`.

- [ ] **Step 4a: Add `agent_id` and `agent_type` to `HookPayload`**

In `pebble-app/src-tauri/src/adapter/mod.rs`, add to `HookPayload`:

```rust
pub agent_id: Option<String>,
pub agent_type: Option<String>,
```

- [ ] **Step 5: Run tests**

```bash
cd pebble-app/src-tauri && cargo test --lib adapter::claude::tests -- --nocapture
```

Expected: `test_subagent_start_stop_lifecycle ... ok`

- [ ] **Step 6: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/mod.rs pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "feat(adapter): handle SubagentStart/SubagentStop and timeout fallback"
```

---

### Task 4: Register SubagentStart/SubagentStop Hooks

**Files:**
- Modify: `pebble-app/src-tauri/src/hook/bridge.rs`
- Test: manual verification via `cat ~/.claude/settings.json`

**Context:** The bridge auto-configures hooks. We need to append `SubagentStart` and `SubagentStop` without overwriting other tools.

- [ ] **Step 1: Add hook entries**

In `pebble-app/src-tauri/src/hook/bridge.rs`, inside the `pebble_hooks` JSON object, add two lines after `SessionStart`:

```rust
"SubagentStart": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} SubagentStart", cmd) }] }],
"SubagentStop": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} SubagentStop", cmd) }] }],
```

The full `pebble_hooks` block should now contain:
- `UserPromptSubmit`
- `PreToolUse`
- `PostToolUse`
- `PostToolUseFailure`
- `PermissionRequest`
- `Stop`
- `SessionStart`
- `SubagentStart`
- `SubagentStop`

- [ ] **Step 2: Verify the append-only logic**

Confirm that the existing loop:

```rust
for (key, value) in pebble_hooks.as_object().unwrap() {
    if existing_hooks.get(key) != Some(value) {
        existing_hooks.insert(key.clone(), value.clone());
        changed = true;
    }
}
```

...is already append-only. It only overwrites if the **value differs**. Since vibe-island uses a different command string, our value will differ and our entry will be inserted. If another tool wrote the exact same command, it would be a no-op. This is correct.

- [ ] **Step 3: Run a manual test**

Build the app to trigger auto-configure, then inspect settings:

```bash
cd pebble-app/src-tauri && cargo build
# After build, check:
grep -A5 '"SubagentStart"' ~/.claude/settings.json
```

Expected: contains `pebble-bridge SubagentStart`.

```bash
grep -A5 '"SubagentStop"' ~/.claude/settings.json
```

Expected: contains `pebble-bridge SubagentStop`.

Also confirm vibe-island entries are still present:

```bash
grep -c "vibe-island-bridge" ~/.claude/settings.json
```

Expected: count > 0.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/hook/bridge.rs
git commit -m "feat(hook): add SubagentStart/SubagentStop to auto-configured hooks"
```

---

### Task 5: Forward agent_id/agent_type in Main Hook Handler

**Files:**
- Modify: `pebble-app/src-tauri/src/main.rs`
- Test: compile check

**Context:** The raw `IncomingHookPayload` must be mapped to `HookPayload` so the adapter sees `agent_id` and `agent_type`.

- [ ] **Step 1: Map the two fields**

In `pebble-app/src-tauri/src/main.rs`, inside the hook server closure, find the `hook_payload = adapter::HookPayload { ... }` block. Add:

```rust
agent_id: payload.agent_id.clone(),
agent_type: payload.agent_type.clone(),
```

Place them after `wezterm_unix_socket`.

- [ ] **Step 2: Verify compilation**

```bash
cd pebble-app/src-tauri && cargo check
```

Expected: clean compile.

- [ ] **Step 3: Commit**

```bash
git add pebble-app/src-tauri/src/main.rs
git commit -m "feat(main): forward agent_id and agent_type to adapter"
```

---

### Task 6: Frontend Collapsible Subagent List

**Files:**
- Modify: `pebble-app/src/App.tsx`

**Context:** The UI currently always renders all subagents. We want a single summary line that expands on click.

- [ ] **Step 1: Add `expandedSubagents` state to `InstanceCard`**

Inside `InstanceCard`, after `const [responding, setResponding] = useState(false);`, add:

```tsx
const [expandedSubagents, setExpandedSubagents] = useState(false);
```

- [ ] **Step 2: Replace the subagents rendering block**

Find this block (around line 316):

```tsx
{inst.subagents.length > 0 && (
  <div className="subagents">
    <div className="subagents-title">Subagents ({inst.subagents.length})</div>
    <div className="subagents-list">
      {inst.subagents.map((sub) => (
        <div
          key={sub.id}
          className={`subagent subagent--${sub.status}`}
          onClick={() => onSubagentClick?.(sub.id)}
        >
          <StatusDot status={sub.status} />
          <span className="subagent-name">{sub.name}</span>
          <span className="subagent-status">{sub.status === "completed" ? "Done" : sub.status}</span>
        </div>
      ))}
    </div>
  </div>
)}
```

Replace it with:

```tsx
{inst.subagents.length > 0 && (
  <div className="subagents">
    <div
      className="subagents-title subagents-title--clickable"
      onClick={(e) => {
        e.stopPropagation();
        setExpandedSubagents((v) => !v);
      }}
      title={inst.subagents.map((s) => `${s.name} ${s.status}`).join("; ")}
    >
      Subagents ({inst.subagents.length}) {expandedSubagents ? "▲" : "▼"}
    </div>
    {expandedSubagents && (
      <div className="subagents-list">
        {inst.subagents.map((sub) => {
          const fullText = `${sub.name} ${sub.status}`;
          return (
            <div
              key={sub.id}
              className={`subagent subagent--${sub.status}`}
              onClick={() => onSubagentClick?.(sub.id)}
              title={fullText}
            >
              <StatusDot status={sub.status} />
              <span className="subagent-name">{sub.name}</span>
              <span className="subagent-status">{sub.status === "completed" ? "Done" : sub.status}</span>
            </div>
          );
        })}
      </div>
    )}
  </div>
)}
```

- [ ] **Step 3: Add CSS cursor for clickable title**

In `pebble-app/src/App.css` (or wherever subagent styles live), find `.subagents-title` and add:

```css
.subagents-title--clickable {
  cursor: pointer;
  user-select: none;
}
```

If `App.css` does not exist or the class isn't there, search for it:

```bash
grep -r "subagents-title" pebble-app/src/
```

If the CSS is in `App.css`, modify it. If it's inline or absent, just add the rule at the bottom of `App.css`.

- [ ] **Step 4: Type-check the frontend**

```bash
cd pebble-app && npx tsc --noEmit
```

Expected: no TypeScript errors.

- [ ] **Step 5: Commit**

```bash
git add pebble-app/src/App.tsx pebble-app/src/App.css
git commit -m "feat(ui): collapsible subagent list with tooltips"
```

---

### Task 7: Integration Verification

**Files:** N/A (whole-system verification)

- [ ] **Step 1: Run full Rust test suite**

```bash
cd pebble-app/src-tauri && cargo test --lib
```

Expected: all tests pass (existing + new).

- [ ] **Step 2: Build the Tauri app**

```bash
cd pebble-app/src-tauri && cargo build
```

Expected: successful build.

- [ ] **Step 3: Verify frontend builds**

```bash
cd pebble-app && npm run build
```

Expected: Vite build completes without errors.

- [ ] **Step 4: Final commit if any changes**

If you made any fixes during integration, commit them now.

---

## Self-Review

**Spec coverage:**
- Hook auto-config (append-only) → Task 4
- Payload schema extensions → Tasks 1, 5
- Adapter state tracking (`SubagentState`) → Task 1
- Hook handling (`SubagentStart`/`SubagentStop`) → Task 3
- File fallback + `.jsonl` mtime → Task 2, Task 3
- Timeout cleanup (600s) → Task 3
- UI display format (`agent_type description status`) → Task 3 (`full_name`)
- Collapsible subagent list with tooltips → Task 6

**Placeholder scan:** All code blocks contain concrete implementation. No TBDs or vague steps.

**Type consistency:**
- `IncomingHookPayload` fields: `agent_id`, `agent_type`
- `HookPayload` fields: `agent_id`, `agent_type`
- `SubagentState` fields: `id`, `name`, `description`, `started_at`
- `list_subagents_with_mtime` returns `(SubagentMeta, u64)`
- All usages match these definitions.

**Gap check:** None found. Every design section maps to a task.
