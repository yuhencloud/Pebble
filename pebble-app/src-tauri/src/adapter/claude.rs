use crate::adapter::{Adapter, AdapterState, HookPayload, RawInstance};
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
            let session = session::read_session_for_pid(proc.pid);
            let cwd = session.as_ref().map(|s| s.cwd.clone())
                .or_else(|| platform::cwd::get_process_cwd(proc.pid))
                .unwrap_or_else(|| "Unknown".to_string());
            let session_name = session.as_ref().and_then(|s| s.name.clone());
            let terminal = platform::terminal::detect_terminal_app(proc.pid, &ps_output);
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
        if let Some(ref session_id) = state.transcript_path {
            let sid = std::path::Path::new(session_id)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(session_id);
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

