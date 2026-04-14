use crate::adapter::{Adapter, AdapterState, HookPayload, RawInstance, SubagentState};
use crate::platform;
use crate::session;
use crate::transcript;
use crate::types::{Instance, PendingPermission, SubagentInfo};
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
        let bridge_path = crate::hook::bridge::ensure_bridge_binary();
        crate::hook::bridge::ensure_claude_hooks_config(&bridge_path);
        Ok(())
    }

    fn discover_instances(&self) -> Vec<RawInstance> {
        let claudes = platform::discovery::find_claude_processes();
        let mut results = Vec::new();

        for proc in claudes {
            let session = session::read_session_for_pid(proc.pid);
            let cwd = session.as_ref().map(|s| s.cwd.clone())
                .or_else(|| platform::cwd::get_process_cwd(proc.pid))
                .unwrap_or_else(|| "Unknown".to_string());
            let session_name = session.as_ref().and_then(|s| s.name.clone());
            let terminal = platform::terminal::detect_terminal_app(proc.pid);
            let id = format!("cc-{}", proc.pid);

            results.push(RawInstance {
                id,
                pid: proc.pid,
                working_directory: cwd,
                terminal_app: terminal,
                session_name,
            });
        }

        results
    }

    fn handle_hook(
        &self,
        payload: &HookPayload,
        state: &mut AdapterState,
        _instances: &mut HashMap<String, Instance>,
    ) {
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
        Self::update_opt_string(&mut state.wezterm_pane_id, payload.wezterm_pane_id.as_ref());
        Self::update_opt_string(&mut state.wt_session_id, payload.wt_session_id.as_ref());
        Self::update_opt_string(&mut state.wezterm_unix_socket, payload.wezterm_unix_socket.as_ref());
        if let Some(ref m) = payload.model {
            state.model = Some(m.clone());
        }
        if let Some(cp) = payload.context_percent {
            state.context_percent = Some(cp);
        }

        let event = crate::types::HookEvent {
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

        if payload.event == "PermissionRequest" || is_permission_event {
            state.status = "needs_permission".to_string();
            let tool_name = payload.tool_name.clone().unwrap_or_else(|| "Claude".to_string());
            let is_dangerous = matches!(
                tool_name.as_str(),
                "Bash" | "Edit" | "Write" | "Read" | "MultiEdit" | "Delete"
            );
            let choices = payload.choices.clone().unwrap_or_else(|| {
                if is_dangerous {
                    vec![
                        "Allow for this conversation".to_string(),
                        "Allow once".to_string(),
                        "Deny".to_string(),
                    ]
                } else {
                    vec!["Allow".to_string(), "Deny".to_string()]
                }
            });
            let default_choice = payload.default_choice.clone().or_else(|| {
                if is_dangerous {
                    Some("Allow once".to_string())
                } else {
                    Some("Allow".to_string())
                }
            });
            state.pending_permission = Some(PendingPermission {
                tool_name: tool_name.clone(),
                tool_use_id: payload.tool_use_id.clone().unwrap_or_else(|| payload.timestamp.to_string()),
                prompt: format!("Allow {}?", tool_name),
                choices,
                default_choice,
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
                    let fallback = input.to_string();
                    let text = input.as_str().unwrap_or(&fallback);
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
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // 1. Timeout cleanup
        let mut cleaned = state.subagents.clone();
        const SUBAGENT_TIMEOUT_SECS: u64 = 600;
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

    fn jump_to_terminal(&self, instance: &Instance) -> Result<(), String> {
        platform::jump::jump_to_terminal(
            instance.pid,
            &instance.terminal_app,
            instance.wezterm_pane_id.as_deref(),
            instance.wt_session_id.as_deref(),
            instance.wezterm_unix_socket.as_deref(),
        )
    }

    fn respond_permission(
        &self,
        instance: &Instance,
        decision: &str,
        reason: Option<&str>,
    ) -> Result<String, String> {
        let trimmed = decision.trim();
        let behavior = Self::normalize_permission_choice(trimmed)?;
        let is_pretooluse = instance
            .last_hook_event
            .as_ref()
            .map(|e| e.event == "PreToolUse")
            .unwrap_or(false);

        if is_pretooluse {
            let mut hso = serde_json::json!({
                "hookEventName": "PreToolUse",
                "permissionDecision": behavior
            });
            if let Some(r) = reason {
                hso["permissionDecisionReason"] = serde_json::json!(r);
            }
            Ok(serde_json::json!({
                "continue": true,
                "hookSpecificOutput": hso
            }).to_string())
        } else {
            let mut decision_obj = serde_json::json!({ "behavior": behavior });
            if behavior == "deny" {
                decision_obj["message"] = serde_json::json!(reason.unwrap_or("Denied by user via Pebble"));
            }
            Ok(serde_json::json!({
                "continue": true,
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": decision_obj
                }
            }).to_string())
        }
    }
}

impl ClaudeAdapter {
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

    fn normalize_permission_choice(choice: &str) -> Result<&'static str, String> {
        if choice.eq_ignore_ascii_case("allow")
            || choice.eq_ignore_ascii_case("allow for this conversation")
            || choice.eq_ignore_ascii_case("allow once")
        {
            Ok("allow")
        } else if choice.eq_ignore_ascii_case("deny") {
            Ok("deny")
        } else {
            Err(format!("Invalid permission choice: {}", choice))
        }
    }
}

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

