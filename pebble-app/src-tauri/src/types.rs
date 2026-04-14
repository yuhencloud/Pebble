use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

#[derive(Serialize, Clone, Debug)]
pub struct PendingPermission {
    pub tool_name: String,
    pub tool_use_id: String,
    pub prompt: String,
    pub choices: Vec<String>,
    pub default_choice: Option<String>,
    pub details: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SubagentInfo {
    pub id: String,
    pub status: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Instance {
    pub id: String,
    pub pid: u32,
    pub status: String,
    pub working_directory: String,
    pub terminal_app: String,
    pub last_activity: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_permission: Option<PendingPermission>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_hook_event: Option<HookEvent>,
    #[serde(default)]
    pub subagents: Vec<SubagentInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_percent: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub conversation_log: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_start: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wezterm_pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wt_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wezterm_unix_socket: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HookEvent {
    pub event: String,
    pub cwd: String,
    pub timestamp: u64,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default, rename = "tool_input")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default, rename = "permission_mode")]
    pub permission_mode: Option<String>,
    #[serde(default, rename = "tool_use_id")]
    pub tool_use_id: Option<String>,
    #[serde(default, rename = "model")]
    pub model: Option<String>,
    #[serde(default, rename = "context_percent")]
    pub context_percent: Option<u8>,
    #[serde(default, rename = "session_name")]
    pub session_name: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct IncomingHookPayload {
    pub event: String,
    pub cwd: String,
    pub timestamp: u64,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default, rename = "tool_input")]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default, rename = "permission_mode")]
    pub permission_mode: Option<String>,
    #[serde(default, rename = "tool_use_id")]
    pub tool_use_id: Option<String>,
    #[serde(default, rename = "model")]
    pub raw_model: Option<serde_json::Value>,
    #[serde(default, rename = "context_percent")]
    pub context_percent: Option<u8>,
    #[serde(default, rename = "context_window")]
    pub context_window: Option<serde_json::Value>,
    #[serde(default, rename = "transcript_path")]
    pub transcript_path: Option<String>,
    #[serde(default, rename = "session_name")]
    pub session_name: Option<String>,
    #[serde(default, rename = "sender_pid")]
    pub sender_pid: Option<u32>,
    #[serde(default, rename = "choices")]
    pub choices: Option<Vec<String>>,
    #[serde(default, rename = "default_choice")]
    pub default_choice: Option<String>,
    #[serde(default, rename = "wezterm_pane_id")]
    pub wezterm_pane_id: Option<String>,
    #[serde(default, rename = "wt_session_id")]
    pub wt_session_id: Option<String>,
    #[serde(default, rename = "wezterm_unix_socket")]
    pub wezterm_unix_socket: Option<String>,
    #[serde(default, rename = "agent_id")]
    pub agent_id: Option<String>,
    #[serde(default, rename = "agent_type")]
    pub agent_type: Option<String>,
}

pub struct AppState {
    pub instances: Arc<Mutex<HashMap<String, Instance>>>,
    pub registry: crate::adapter::AdapterRegistry,
    pub adapter_states: Arc<Mutex<HashMap<String, crate::adapter::AdapterState>>>,
}

