use std::io::Read;
use sysinfo::{ProcessStatus, System};

fn main() {
    if run().is_err() {
        std::process::exit(0);
    }
}

/// Query `wezterm cli list --format json` to find the pane whose cwd matches ours.
fn detect_wezterm_pane(cwd: &str) -> Option<String> {
    let mut cmd = std::process::Command::new("wezterm");
    cmd.args(["cli", "list", "--format", "json"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let panes: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).ok()?;
    let cwd_normalized = normalize_path(cwd);

    // Find the pane whose cwd matches our working directory
    for pane in &panes {
        let pane_cwd = pane.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
        // WezTerm returns cwd as file:///path/
        let pane_path = pane_cwd
            .strip_prefix("file:///").unwrap_or(pane_cwd)
            .trim_end_matches('/');
        if normalize_path(pane_path) == cwd_normalized {
            if let Some(id) = pane.get("pane_id").and_then(|v| v.as_u64()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Detect wezterm unix socket by finding the gui-sock file.
fn detect_wezterm_socket() -> Option<String> {
    let home = dirs::home_dir()?;
    let sock_dir = home.join(".local/share/wezterm");
    if sock_dir.is_dir() {
        for entry in std::fs::read_dir(&sock_dir).ok()? {
            if let Ok(entry) = entry {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("gui-sock-") {
                    return Some(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let event_type = std::env::args().nth(1).unwrap_or_else(|| "unknown".to_string());
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis() as u64;

    let mut stdin_data = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_data);

    let my_pid = std::process::id() as usize;
    let mut current_pid = my_pid;
    let mut sender_pid = None::<u32>;

    let s = System::new_all();

    loop {
        if let Some(proc) = s.process(sysinfo::Pid::from(current_pid)) {
            let raw_name = proc.name();
            let name = raw_name.strip_suffix(".exe").unwrap_or(raw_name);
            let status = proc.status();
            let args = proc.cmd().join(" ");
            let is_claude = name == "claude" || name == "claude-code";
            let is_node_claude = name == "node" && args.contains("claude-code");
            if status != ProcessStatus::Zombie && (is_claude || is_node_claude) {
                sender_pid = Some(current_pid as u32);
                break;
            }
            match proc.parent() {
                Some(ppid) if ppid.as_u32() != 0 && ppid.as_u32() != current_pid as u32 => {
                    current_pid = ppid.as_u32() as usize;
                }
                _ => break,
            }
        } else {
            break;
        }
    }

    let mut body = serde_json::json!({
        "event": event_type,
        "cwd": cwd,
        "timestamp": timestamp,
    });
    if let Some(pid) = sender_pid {
        body["sender_pid"] = serde_json::json!(pid);
    }
    // Try WEZTERM_PANE env var first; fall back to querying wezterm cli
    let wezterm_pane = std::env::var("WEZTERM_PANE").ok().filter(|v| !v.trim().is_empty());
    let wezterm_pane = wezterm_pane.or_else(|| detect_wezterm_pane(&cwd));
    if let Some(pane) = &wezterm_pane {
        body["wezterm_pane_id"] = serde_json::json!(pane.trim());
    }
    if let Some(session) = std::env::var("WT_SESSION").ok() {
        if !session.trim().is_empty() {
            body["wt_session_id"] = serde_json::json!(session.trim());
        }
    }
    let wezterm_sock = std::env::var("WEZTERM_UNIX_SOCKET").ok().filter(|v| !v.trim().is_empty());
    let wezterm_sock = wezterm_sock.or_else(|| detect_wezterm_socket());
    if let Some(sock) = &wezterm_sock {
        body["wezterm_unix_socket"] = serde_json::json!(sock.trim());
    }
    let stdin_trimmed = stdin_data.trim();
    if !stdin_trimmed.is_empty() {
        match serde_json::from_str::<serde_json::Value>(stdin_trimmed) {
            Ok(parsed) => {
                if let Some(obj) = parsed.as_object() {
                    let mut merged = obj.clone();
                    for (k, v) in body.as_object().unwrap() {
                        merged.insert(k.clone(), v.clone());
                    }
                    body = serde_json::Value::Object(merged);
                } else {
                    body["stdin"] = serde_json::json!(stdin_data);
                }
            }
            Err(_) => {
                body["stdin"] = serde_json::json!(stdin_data);
            }
        }
    }

    let timeout = if event_type == "PermissionRequest" {
        std::time::Duration::from_secs(300)
    } else {
        std::time::Duration::from_millis(500)
    };

    let payload = serde_json::to_string(&body)?;
    let response = ureq::post("http://127.0.0.1:9876/hook")
        .set("Content-Type", "application/json")
        .timeout(timeout)
        .send_string(&payload)?;

    let mut response_body = String::new();
    response.into_reader().read_to_string(&mut response_body)?;

    if !response_body.trim().is_empty() {
        println!("{}", response_body.trim());
    }
    Ok(())
}
