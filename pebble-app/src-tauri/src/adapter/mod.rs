pub mod claude;

use crate::types::{Instance, SubagentInfo};
use std::collections::HashMap;
use std::sync::Arc;


#[derive(Debug, Clone)]
pub struct RawInstance {
    pub id: String,
    pub pid: u32,
    pub working_directory: String,
    pub terminal_app: String,
    pub session_name: Option<String>,
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
    pub choices: Option<Vec<String>>,
    pub default_choice: Option<String>,
    pub wezterm_pane_id: Option<String>,
    pub wt_session_id: Option<String>,
    pub wezterm_unix_socket: Option<String>,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SubagentState {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub started_at: u64,
}

/// Mutable state held per instance by the adapter
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
}

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
    pub adapters: Vec<Arc<dyn Adapter>>,
}

impl Clone for AdapterRegistry {
    fn clone(&self) -> Self {
        Self {
            adapters: self.adapters.iter().map(|a| Arc::clone(a)).collect(),
        }
    }
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self { adapters: Vec::new() }
    }

    pub fn register(&mut self, adapter: Arc<dyn Adapter>) {
        self.adapters.push(adapter);
    }

    pub fn configure_all(&self) -> Vec<Result<(), String>> {
        self.adapters.iter().map(|a| a.auto_configure()).collect()
    }

    pub fn discover_all(&self) -> Vec<RawInstance> {
        self.adapters.iter().flat_map(|a: &Arc<dyn Adapter>| a.discover_instances()).collect()
    }

    pub fn find_adapter_for_event<'a>(&'a self, _payload: &HookPayload) -> Option<&'a dyn Adapter> {
        // For now, all events are assumed to be from Claude
        self.adapters.first().map(|a: &Arc<dyn Adapter>| a.as_ref())
    }
}
