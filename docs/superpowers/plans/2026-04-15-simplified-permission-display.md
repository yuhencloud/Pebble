# Simplified Permission Display Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove interactive permission buttons from Pebble UI and add automatic state cleanup when users act in the Claude terminal.

**Architecture:** Pebble becomes a read-only alert surface: red styling shows pending permissions, clicking anywhere jumps to the terminal, and backend clears stale `pending_permission` on new hook events or terminal jumps.

**Tech Stack:** React + TypeScript (frontend), Rust + Tauri (backend).

---

## File Map

| File | Responsibility |
|------|----------------|
| `pebble-app/src/App.tsx` | React UI: remove permission buttons, keep red alert styling, remove `onRespond` flow |
| `pebble-app/src-tauri/src/adapter/claude.rs` | Backend hook handler: clear `pending_permission` when subsequent non-permission events arrive |
| `pebble-app/src-tauri/src/main.rs` | Tauri commands: clear `pending_permission` inside `jump_to_terminal`; unregister `respond_permission` command |
| `pebble-app/src-tauri/src/adapter/mod.rs` | Trait definition: remove `respond_permission` from `Adapter` trait |

---

### Task 1: Remove permission response from Rust trait and adapter

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/mod.rs`
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs`

- [ ] **Step 1: Remove `respond_permission` from `Adapter` trait**

In `pebble-app/src-tauri/src/adapter/mod.rs`, delete the `respond_permission` method from the `Adapter` trait (lines 122-128):

```rust
    /// Respond to a permission request (for hooks that support it)
    fn respond_permission(
        &self,
        instance: &Instance,
        decision: &str,
        reason: Option<&str>,
    ) -> Result<String, String>;
```

- [ ] **Step 2: Remove `respond_permission` and `normalize_permission_choice` from `ClaudeAdapter`**

In `pebble-app/src-tauri/src/adapter/claude.rs`, delete the entire `respond_permission` method implementation (lines 334-382) and the `normalize_permission_choice` helper method (lines 399-410).

- [ ] **Step 3: Run `cargo check`**

```bash
cd pebble-app/src-tauri && cargo check
```

Expected: compilation errors in `main.rs` because `respond_permission` is still referenced there. This is expected and will be fixed in Task 3.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/mod.rs pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "refactor(adapter): remove respond_permission from trait and ClaudeAdapter"
```

---

### Task 2: Add auto-clear logic to hook handler

**Files:**
- Modify: `pebble-app/src-tauri/src/adapter/claude.rs`

- [ ] **Step 1: Add auto-clear helper method to `ClaudeAdapter`**

Append this private helper to the `impl ClaudeAdapter` block (after `update_opt_string`):

```rust
    fn clear_stale_permission(state: &mut AdapterState) {
        if state.pending_permission.is_some() {
            state.pending_permission = None;
        }
    }
```

- [ ] **Step 2: Use `clear_stale_permission` in the non-permission branch of `handle_hook`**

In `handle_hook`, find the existing code near the bottom:

```rust
        } else {
            let new_status = match payload.event.as_str() {
                "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => "executing",
                _ => "waiting",
            };
            state.status = new_status.to_string();
            state.pending_permission = None;
        }
```

Change `state.pending_permission = None;` to:

```rust
            Self::clear_stale_permission(state);
```

This single change already covers `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `SubagentStart`, `SubagentStop`, and all other non-permission events because they all fall into this `else` branch.

- [ ] **Step 3: Run Rust tests**

```bash
cd pebble-app/src-tauri && cargo test
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add pebble-app/src-tauri/src/adapter/claude.rs
git commit -m "feat(adapter): clear stale pending_permission on non-permission hook events"
```

---

### Task 3: Remove `respond_permission` command and add jump-to-terminal clear

**Files:**
- Modify: `pebble-app/src-tauri/src/main.rs`

- [ ] **Step 1: Remove `respond_permission` Tauri command**

In `pebble-app/src-tauri/src/main.rs`, delete the entire `respond_permission` function (lines 87-149).

Also remove `respond_permission` from the `.invoke_handler(...)` macro call (line 791). Change:

```rust
.invoke_handler(tauri::generate_handler![get_instances, jump_to_terminal, respond_permission, get_instance_preview, resize_window_centered, bring_to_front])
```

to:

```rust
.invoke_handler(tauri::generate_handler![get_instances, jump_to_terminal, get_instance_preview, resize_window_centered, bring_to_front])
```

- [ ] **Step 2: Add permission clear inside `jump_to_terminal`**

In `pebble-app/src-tauri/src/main.rs`, the current `jump_to_terminal` function (lines 55-85) is:

```rust
#[tauri::command]
fn jump_to_terminal(instance_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let instance = {
        let map = state.instances.lock();
        map.values()
            .find(|i| i.id == instance_id)
            .cloned()
            .ok_or("Instance not found")?
    };
    let adapter = state.registry.find_adapter_for_event(&adapter::HookPayload {
        event: "discover".to_string(),
        cwd: instance.working_directory.clone(),
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
        agent_id: None,
        agent_type: None,
    }).ok_or("No adapter found")?;
    adapter.jump_to_terminal(&instance)
}
```

Replace it with:

```rust
#[tauri::command]
fn jump_to_terminal(instance_id: String, state: State<'_, AppState>) -> Result<(), String> {
    // Clear stale permission when user explicitly jumps to terminal
    {
        let mut map = state.instances.lock();
        if let Some(inst) = map.values_mut().find(|i| i.id == instance_id) {
            if inst.status == "needs_permission" && inst.pending_permission.is_some() {
                inst.status = "executing".to_string();
                inst.pending_permission = None;
            }
        }
    }

    let instance = {
        let map = state.instances.lock();
        map.values()
            .find(|i| i.id == instance_id)
            .cloned()
            .ok_or("Instance not found")?
    };
    let adapter = state.registry.find_adapter_for_event(&adapter::HookPayload {
        event: "discover".to_string(),
        cwd: instance.working_directory.clone(),
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
        agent_id: None,
        agent_type: None,
    }).ok_or("No adapter found")?;
    adapter.jump_to_terminal(&instance)
}
```

- [ ] **Step 3: Run `cargo check`**

```bash
cd pebble-app/src-tauri && cargo check
```

Expected: clean compilation.

- [ ] **Step 4: Run Rust tests**

```bash
cd pebble-app/src-tauri && cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add pebble-app/src-tauri/src/main.rs
git commit -m "feat(commands): clear pending_permission on jump_to_terminal and remove respond_permission"
```

---

### Task 4: Remove interactive permission buttons from React UI

**Files:**
- Modify: `pebble-app/src/App.tsx`

- [ ] **Step 1: Remove `onRespond` prop from `InstanceCard`**

In `pebble-app/src/App.tsx`, update the `InstanceCard` props interface:

```tsx
function InstanceCard({
  inst,
  onClick,
  onSubagentClick,
}: {
  inst: Instance;
  onClick: () => void;
  onSubagentClick?: () => void;
}) {
```

Delete the `onRespond` parameter and the `responding` state inside `InstanceCard`:

```tsx
  const [responding, setResponding] = useState(false);
```

Delete the `handleRespond` function:

```tsx
  const handleRespond = async (choice: string) => {
    if (responding || !onRespond) return;
    setResponding(true);
    try {
      await onRespond(choice);
    } finally {
      setResponding(false);
    }
  };
```

- [ ] **Step 2: Replace permission buttons with read-only alert text**

Find the permission card block in `InstanceCard`:

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

Replace it with:

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
          <div className="permission-hint">
            请在终端中处理此请求
          </div>
        </div>
      )}
```

- [ ] **Step 3: Remove `onRespond` usage from the `App` component**

In the `App` component, delete the `respondPermission` function:

```tsx
  const respondPermission = async (instanceId: string, choice: string) => {
    try {
      await invoke("respond_permission", { instanceId, choice });
      setInstances((prev) =>
        prev.map((i) =>
          i.id === instanceId
            ? { ...i, status: "executing", pending_permission: undefined }
            : i
        )
      );
    } catch (e) {
      console.error("Failed to respond:", e);
    }
  };
```

Update the `InstanceCard` render call to remove `onRespond`:

```tsx
            {realInstances.map((inst) => (
              <InstanceCard
                key={inst.id}
                inst={inst}
                onClick={() => jumpToTerminal(inst.id)}
                onSubagentClick={() => jumpToTerminal(inst.id)}
              />
            ))}
```

- [ ] **Step 4: Add CSS for the hint text**

In `pebble-app/src/App.css`, add a new rule near the existing permission styles:

```css
.permission-hint {
  font-size: 12px;
  color: #ff8c42;
  margin-top: 8px;
  text-align: center;
}
```

- [ ] **Step 5: Verify frontend compiles**

Run:

```bash
cd pebble-app && npm run build
```

Expected: build completes without TypeScript or Vite errors.

- [ ] **Step 6: Commit**

```bash
git add pebble-app/src/App.tsx pebble-app/src/App.css
git commit -m "feat(ui): remove permission buttons, show read-only alert with hint"
```

---

## Self-Review Checklist

**Spec coverage:**
- Remove interactive buttons: Task 4 covers this.
- Keep red alert styling: Task 4 preserves the `permission-card` and `status === "needs_permission"` styling.
- Auto-clear on new hook events (Strategy B): Task 2 covers this.
- Auto-clear on terminal jump (Strategy C): Task 3 Step 2 covers this.

**Placeholder scan:**
- No TODOs, TBDs, or vague instructions.
- Every step contains exact code blocks and commands.

**Type consistency:**
- `pending_permission` is set to `None` in Rust and `undefined` in TS, matching existing patterns.
- `status` strings match the existing `Instance` type union.
