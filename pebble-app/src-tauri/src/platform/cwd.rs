use std::process::Command;
use std::path::Path;

pub fn get_process_cwd(pid: u32) -> Option<String> {
    if pid == 0 { return None; }
    // Try session file first (cross-platform)
    let session_path = dirs::home_dir()?.join(".claude").join("sessions").join(format!("{}.json", pid));
    if let Ok(content) = std::fs::read_to_string(&session_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(cwd) = json.get("cwd").and_then(|v| v.as_str()) {
                return Some(cwd.to_string());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("lsof")
            .args(["-a", "-d", "cwd", "-p", &pid.to_string(), "-Fn"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with('n') {
                return Some(line[1..].to_string());
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let link = format!("/proc/{}/cwd", pid);
        if let Ok(path) = std::fs::read_link(&link) {
            return path.to_str().map(|s| s.to_string());
        }
    }

    None
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
