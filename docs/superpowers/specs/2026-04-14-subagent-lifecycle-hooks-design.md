# Subagent Lifecycle Hooks (SubagentStart / SubagentStop)

## Problem

Currently, Pebble lists subagents by scanning the `~/.claude/projects/.../subagents/` directory for `.meta.json` files. However, the code hard-codes every discovered subagent's status as `"executing"`. Once a subagent finishes, there is no signal to update or remove it from the UI, so completed subagents appear to run forever.

Additionally, the `.meta.json` files contain no completion timestamp or status field, so it is impossible to determine whether a subagent has finished by reading files alone.

## Goal

Add precise, hook-driven subagent lifecycle tracking so that:
- Subagents appear in the UI as soon as they start
- Subagents are removed from the UI when they stop
- Subagents that never receive a `SubagentStop` hook are cleaned up after a timeout
- If Pebble starts after a subagent has already begun, the file-based fallback still shows it
- The UI displays richer subagent information (type + description + status)

## Design

### 1. Hook Auto-Configuration

In `hook/bridge.rs`, add `SubagentStart` and `SubagentStop` to the auto-generated hooks configuration so that `pebble-bridge` is invoked whenever a subagent starts or stops.

```json
"SubagentStart": [
  { "matcher": "*", "hooks": [{ "type": "command", "command": "<bridge> SubagentStart" }] }
],
"SubagentStop": [
  { "matcher": "*", "hooks": [{ "type": "command", "command": "<bridge> SubagentStop" }] }
]
```

**Important**: When writing to `~/.claude/settings.json`, Pebble must **append** these entries without overwriting existing hooks from other tools (e.g., vibe-island).

### 2. Payload Schema Extensions

`IncomingHookPayload` in `types.rs` gains two new optional fields:
- `agent_id` — unique subagent identifier (e.g., `"ab9219d79ff500de4"`)
- `agent_type` — subagent type (e.g., `"Explore"`, `"Bash"`, `"superpowers:code-reviewer"`)

### 3. Adapter State Tracking

`AdapterState` in `adapter/mod.rs` gains:
```rust
pub struct SubagentState {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub started_at: u64,
}

pub subagents: HashMap<String, SubagentState>
```

The map key is `agent_id`.

### 4. Hook Handling (`ClaudeAdapter::handle_hook`)

| Event | Action |
|-------|--------|
| `SubagentStart` | Insert or update the subagent in `state.subagents` with `started_at = now` |
| `SubagentStop` | Remove the subagent from `state.subagents` by `agent_id` |

If a `SubagentStop` arrives for an unknown `agent_id`, it is a no-op.

### 5. File-Based Fallback + mtime Precision

When Pebble starts (or refreshes instances) and the adapter state has no memory of a subagent, it falls back to scanning the `subagents/` directory:
- For each `agent-<id>.meta.json`, derive `agent_id`
- Read `agentType` and `description`
- For `started_at`, use the modification time (`mtime`) of the corresponding `agent-<id>.jsonl` transcript file. This is more accurate than using the current time because the `.jsonl` is created when the subagent starts.
- If `.jsonl` does not exist, fall back to `.meta.json` mtime, then `now`
- Insert into `state.subagents` only if the key does not already exist

### 6. Timeout Cleanup

A dedicated timeout threshold of **600 seconds (10 minutes)** is applied to subagents. During the state monitor loop (or inside `get_subagents`), any subagent whose `started_at` is older than 600s is removed from the map.

This is independent of the instance-level 30-second executing timeout.

### 7. UI Display Format

Each subagent row renders as:

```
<agent_type> <description> <status>
```

For example:
```
superpowers:code-reviewer 代码质量审查 Task 1 executing
```

If `description` is absent, only the type and status are shown.

### 8. Tauri Event Flow

No frontend protocol changes are required. `get_subagents` continues to return `Vec<SubagentInfo>`, but the `name` field now contains the combined display text (`"{agent_type} {description}"`), and the `status` field remains `"executing"` for active subagents. Completed or timed-out subagents are simply omitted from the vector, so they disappear from the UI.

## Out of Scope

- **Persistent subagent history**: We do not keep completed subagents in the UI.
- **Per-agent-type timeout**: All subagents share the same 600s timeout.
- **File-system watchers**: No `notify` crate or inotify/fsevents integration; refresh relies on the existing 2-second poll plus hook events.

## Files to Modify

1. `pebble-app/src-tauri/src/types.rs` — add `agent_id`, `agent_type` to `IncomingHookPayload`; extend `SubagentInfo` if needed
2. `pebble-app/src-tauri/src/adapter/mod.rs` — add `SubagentState` struct and `subagents` field to `AdapterState`
3. `pebble-app/src-tauri/src/adapter/claude.rs` — handle `SubagentStart`/`SubagentStop` in `handle_hook`; update `get_subagents` with mtime fallback and timeout cleanup
4. `pebble-app/src-tauri/src/hook/bridge.rs` — append `SubagentStart` and `SubagentStop` to auto-configured hooks
5. `pebble-app/src-tauri/src/session.rs` — optionally expose a helper to read `.jsonl` mtime alongside `list_subagents`
6. `pebble-app/src-tauri/src/main.rs` — ensure hook payload mapping forwards `agent_id` and `agent_type`
