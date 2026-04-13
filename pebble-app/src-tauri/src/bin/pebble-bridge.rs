use std::io::Read;
use sysinfo::{ProcessStatus, System};

fn main() {
    if run().is_err() {
        std::process::exit(0);
    }
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
            if status != ProcessStatus::Zombie && (name == "claude" || name == "claude-code") {
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
    if let Some(pane) = std::env::var("WEZTERM_PANE").ok() {
        if !pane.trim().is_empty() {
            body["wezterm_pane_id"] = serde_json::json!(pane.trim());
        }
    }
    if let Some(session) = std::env::var("WT_SESSION").ok() {
        if !session.trim().is_empty() {
            body["wt_session_id"] = serde_json::json!(session.trim());
        }
    }
    if let Some(sock) = std::env::var("WEZTERM_UNIX_SOCKET").ok() {
        if !sock.trim().is_empty() {
            body["wezterm_unix_socket"] = serde_json::json!(sock.trim());
        }
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
