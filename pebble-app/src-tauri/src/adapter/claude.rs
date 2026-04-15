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

            let (choices, default_choice, details) = if tool_name == "AskUserQuestion" {
                if let Some(ref input) = payload.tool_input {
                    if let Some(questions) = input.get("questions").and_then(|q| q.as_array()) {
                        if let Some(q) = questions.first() {
                            let question_text = q.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let header = q.get("header").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let options = q.get("options").and_then(|o| o.as_array()).cloned().unwrap_or_default();
                            let labels: Vec<String> = options.iter()
                                .filter_map(|opt| opt.get("label").and_then(|v| v.as_str()))
                                .map(|s| s.to_string())
                                .collect();
                            let default = labels.first().cloned();
                            let detail_text = if !header.is_empty() && !question_text.is_empty() {
                                format!("{}\n{}", header, question_text)
                            } else if !question_text.is_empty() {
                                question_text
                            } else {
                                header
                            };
                            (labels, default, Some(detail_text))
                        } else {
                            (vec!["Allow".to_string(), "Deny".to_string()], Some("Allow".to_string()), None)
                        }
                    } else {
                        (vec!["Allow".to_string(), "Deny".to_string()], Some("Allow".to_string()), None)
                    }
                } else {
                    (vec!["Allow".to_string(), "Deny".to_string()], Some("Allow".to_string()), None)
                }
            } else {
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
                let details = Self::extract_permission_details(payload.tool_name.as_deref(), payload.tool_input.as_ref());
                (choices, default_choice, details)
            };

            state.pending_permission = Some(PendingPermission {
                tool_name: tool_name.clone(),
                tool_use_id: payload.tool_use_id.clone().unwrap_or_else(|| payload.timestamp.to_string()),
                prompt: format!("Allow {}?", tool_name),
                choices,
                default_choice,
                details,
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
        let mut result = Vec::new();

        if let Some(ref user) = state.latest_user_preview {
            result.push(format!("You: {}", user.chars().take(60).collect::<String>()));
        } else if let Some(ref event) = state.last_hook_event {
            if event.event == "UserPromptSubmit" {
                if let Some(ref input) = event.tool_input {
                    let fallback = input.to_string();
                    let text = input.as_str().unwrap_or(&fallback);
                    let truncated: String = text.chars().take(60).collect();
                    let display = if text.chars().count() > 60 {
                        format!("{}...", truncated)
                    } else {
                        truncated
                    };
                    result.push(format!("You: {}", display));
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

    fn get_subagents(&self, state: &mut AdapterState) -> Vec<SubagentInfo> {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // 1. Timeout cleanup on canonical state
        const SUBAGENT_TIMEOUT_SECS: u64 = 600;
        state.subagents.retain(|_, s| now_secs.saturating_sub(s.started_at) <= SUBAGENT_TIMEOUT_SECS);

        state.subagents.values().cloned().map(|s| {
            let full_name = if let Some(ref d) = s.description {
                format!("{}: {}", s.name, d)
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
        let is_ask_user_question = instance
            .pending_permission
            .as_ref()
            .map(|p| p.tool_name == "AskUserQuestion")
            .unwrap_or(false);
        let behavior = if is_ask_user_question {
            trimmed
        } else {
            Self::normalize_permission_choice(trimmed)?
        };
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

    fn extract_permission_details(tool_name: Option<&str>, tool_input: Option<&serde_json::Value>) -> Option<String> {
        let name = tool_name.unwrap_or("Tool");
        let input = tool_input?;
        match name {
            "AskUserQuestion" => {
                let questions = input.get("questions")?.as_array()?;
                let q = questions.first()?;
                let question_text = q.get("question").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let header = q.get("header").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if !header.is_empty() && !question_text.is_empty() {
                    Some(format!("{}\n{}", header, question_text))
                } else if !question_text.is_empty() {
                    Some(question_text)
                } else {
                    Some(header)
                }
            }
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
            "MultiEdit" => {
                let files = input.get("files")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter()
                        .filter_map(|f| f.get("file_path").and_then(|v| v.as_str()))
                        .collect::<Vec<_>>())
                    .unwrap_or_default();
                if files.is_empty() {
                    Some("MultiEdit: (no files specified)".to_string())
                } else {
                    Some(format!("Files: {}", files.join(", ")))
                }
            }
            _ => {
                let json = input.to_string();
                let preview: String = json.chars().take(200).collect();
                Some(format!("{} params: {}", name, preview))
            }
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

    #[test]
    fn test_subagent_bootstrap_from_transcript_path() {
        let adapter = ClaudeAdapter::new();
        let mut state = AdapterState::default();

        let payload = HookPayload {
            event: "UserPromptSubmit".to_string(),
            cwd: "/tmp".to_string(),
            timestamp: 0,
            tool_name: None,
            tool_input: None,
            permission_mode: None,
            tool_use_id: None,
            model: None,
            context_percent: None,
            session_name: None,
            transcript_path: Some("/tmp/transcript.jsonl".to_string()),
            choices: None,
            default_choice: None,
            wezterm_pane_id: None,
            wt_session_id: None,
            wezterm_unix_socket: None,
            agent_id: None,
            agent_type: None,
        };

        assert!(!state.subagents_bootstrapped);
        adapter.handle_hook(&payload, &mut state, &mut std::collections::HashMap::new());
        assert!(state.subagents_bootstrapped);
        assert_eq!(state.transcript_path, Some("/tmp/transcript.jsonl".to_string()));

        // Second hook with same transcript_path should not re-bootstrap
        let before = state.subagents.clone();
        adapter.handle_hook(&payload, &mut state, &mut std::collections::HashMap::new());
        assert!(state.subagents_bootstrapped);
        assert_eq!(state.subagents, before);
    }
}

