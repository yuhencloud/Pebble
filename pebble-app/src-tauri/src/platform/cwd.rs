use sysinfo::System;

pub fn get_process_cwd(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // Primary: session file (cross-platform)
    let session_path = dirs::home_dir()?.join(".claude").join("sessions").join(format!("{}.json", pid));
    if let Ok(content) = std::fs::read_to_string(&session_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(cwd) = json.get("cwd").and_then(|v| v.as_str()) {
                return Some(cwd.to_string());
            }
        }
    }

    // Fallback: sysinfo (cross-platform)
    let s = System::new_all();
    s.process(sysinfo::Pid::from(pid as usize))
        .and_then(|p| p.cwd())
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_process_cwd_current_process() {
        let cwd = get_process_cwd(std::process::id());
        assert!(cwd.is_some(), "should get current process cwd");
    }
}
