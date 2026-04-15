# Simplified Permission Display Design

**Date:** 2026-04-15  
**Status:** Design Approved  
**Related:** Pebble permission UI and hook state synchronization

## Problem Statement

Currently Pebble renders interactive permission buttons for `PermissionRequest` and `PreToolUse` events. However, there are two fundamental issues:

1. **Incomplete coverage:** Not all user-interaction scenarios (e.g. `Elicitation`, complex multi-select `AskUserQuestion`) can be accurately reproduced inside Pebble's small UI surface.
2. **State desync:** If the user responds directly inside the Claude terminal, Claude Code does not emit a follow-up hook to notify Pebble that the permission was resolved. Pebble's red "needs_permission" state can therefore get stuck indefinitely.

## Design Goal

Simplify Pebble's role to **visual notification + one-click terminal jump**, removing all in-Pebble interactive buttons. Add automatic state cleanup so the red alert disappears promptly when the user acts in the terminal.

## Architecture

### Frontend Changes

- **Remove** the `permission-choices` button block and the `onRespond` callback from `InstanceCard`.
- **Keep** the red styling (`status === "needs_permission"`) as the primary alert mechanism.
- **Keep** the text summary showing `tool_name` and `details` so the user knows what is pending.
- **Unify interaction:** clicking anywhere on the card (including the alert area) triggers `jump_to_terminal`.

### Backend Auto-Clear — Strategy B (New Hook Event Overwrite)

In `ClaudeAdapter::handle_hook`, clear any stale `pending_permission` when a subsequent non-permission hook arrives for the same instance. Events that should trigger cleanup:

- `UserPromptSubmit`
- `PreToolUse` where the call does **not** require permission blocking
- `PostToolUse`
- `PostToolUseFailure`
- `Stop`
- `SubagentStart`
- `SubagentStop`

When any of these fire, set:
```rust
state.pending_permission = None;
state.status = /* "executing" or "waiting" depending on event */;
```

This guarantees that as soon as Claude resumes work, Pebble reflects the new state.

### Backend Auto-Clear — Strategy C (Jump-to-Terminal Clear)

In the `jump_to_terminal` Tauri command (`main.rs`), if the target instance currently has `status == "needs_permission"` and a non-empty `pending_permission`:

1. Clear `pending_permission`.
2. Set `status = "executing"`.
3. Emit `instances-updated` so the UI removes the red alert immediately.

Rationale: the user explicitly chose to switch to the terminal; we interpret that as an intent to handle the prompt inside Claude.

### Code Retention / Removal

- **Retain** the `permission_store` and HTTP blocking loop in `hook/server.rs`. It is harmless and may be useful for future features.
- **Remove** the `respond_permission` Tauri command and its frontend call sites, since Pebble no longer sends permission decisions back to Claude.

## Edge Cases

| Scenario | Expected Behavior |
|----------|-------------------|
| User clicks Pebble card while red | Jumps to terminal; red clears instantly |
| User answers inside Claude terminal | Next hook event (e.g. `PostToolUse`) clears red in Pebble |
| Claude sits idle waiting for permission | Red stays until user acts |
| Multiple permission events in rapid succession | Latest permission event wins; previous one is overwritten |

## Out of Scope

- `Elicitation` / `ElicitationResult` hooks: not registered yet; will be addressed when a concrete MCP use case appears.
- Multi-select UI for `AskUserQuestion`: no longer needed because all interaction happens in Claude.
- `PermissionDenied` hook: can be added later as a simple registration change if desired.
