use serde::Deserialize;

use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub started_at: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

pub fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
        .join("sessions")
}

pub fn read_session_for_pid(pid: u32) -> Option<SessionInfo> {
    let path = sessions_dir().join(format!("{}.json", pid));
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn list_all_sessions() -> Vec<SessionInfo> {
    let dir = sessions_dir();
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") {
                    if let Ok(pid) = name.trim_end_matches(".json").parse::<u32>() {
                        if let Some(info) = read_session_for_pid(pid) {
                            results.push(info);
                        }
                    }
                }
            }
        }
    }
    results
}

#[derive(Debug, Clone)]
pub struct SubagentMeta {
    pub agent_id: String,
    pub agent_type: String,
    pub description: Option<String>,
}

pub fn list_subagents(cwd: &str, session_id: &str) -> Vec<SubagentMeta> {
    let project_dir = cwd.replace("\\", "-").replace("/", "-").replace(":", "-").replace(".", "-");
    let subagents_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
        .join("projects")
        .join(&project_dir)
        .join(&session_id)
        .join("subagents");

    let mut results = Vec::new();
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
                        results.push(SubagentMeta {
                            agent_id,
                            agent_type,
                            description,
                        });
                    }
                }
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sessions_dir_exists() {
        let dir = sessions_dir();
        assert!(dir.exists() || dir.parent().map(|p| p.exists()).unwrap_or(false));
    }
}
