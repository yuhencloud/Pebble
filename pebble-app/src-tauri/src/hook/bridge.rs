use std::fs;
use std::path::PathBuf;

fn bridge_exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "pebble-bridge.exe"
    } else {
        "pebble-bridge"
    }
}

/// Locates the pebble-bridge executable bundled with the app.
/// In dev: target/{debug,release}/pebble-bridge
/// In production bundle: next to the main Pebble binary.
fn bundled_bridge_path() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let candidate = exe_dir.join(bridge_exe_name());
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

/// Ensures `~/.pebble/bin/pebble-bridge` exists and is up-to-date.
pub fn ensure_bridge_binary() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let bin_dir = home.join(".pebble").join("bin");
    let target_path = bin_dir.join(bridge_exe_name());

    let source_path = match bundled_bridge_path() {
        Some(p) => p,
        None => return target_path,
    };

    let should_copy = match (fs::metadata(&target_path), fs::metadata(&source_path)) {
        (Ok(t), Ok(s)) => {
            t.len() != s.len()
                || t.modified().ok().zip(s.modified().ok()).map(|(tm, sm)| tm != sm).unwrap_or(true)
        }
        (Err(_), _) => true,
        _ => false,
    };

    if should_copy {
        let _ = fs::create_dir_all(&bin_dir);
        let _ = fs::copy(&source_path, &target_path);
    }

    target_path
}

pub fn ensure_claude_hooks_config(bridge_path: &std::path::Path) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let settings_path = home.join(".claude").join("settings.json");

    let mut settings = match fs::read_to_string(&settings_path) {
        Ok(content) => serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| {
            serde_json::json!({})
        }),
        Err(_) => serde_json::json!({}),
    };

    if !settings.is_object() {
        settings = serde_json::json!({});
    }

    // Use forward slashes so the command works in bash on Windows
    let cmd = bridge_path.to_string_lossy().replace('\\', "/");

    let pebble_hooks = serde_json::json!({
        "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": format!("{} UserPromptSubmit --source claude", cmd) }] }],
        "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PreToolUse --source claude", cmd) }] }],
        "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUse --source claude", cmd) }] }],
        "PostToolUseFailure": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUseFailure --source claude", cmd) }] }],
        "PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PermissionRequest --source claude", cmd), "timeout": 300 }] }],
        "Stop": [{ "hooks": [{ "type": "command", "command": format!("{} Stop --source claude", cmd) }] }],
        "SessionStart": [{ "hooks": [{ "type": "command", "command": format!("{} SessionStart --source claude", cmd) }] }],
        "SubagentStart": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} SubagentStart --source claude", cmd) }] }],
        "SubagentStop": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} SubagentStop --source claude", cmd) }] }]
    });

    let existing_hooks = settings.get("hooks").cloned().unwrap_or(serde_json::json!({}));
    let mut existing_hooks = if existing_hooks.is_object() {
        existing_hooks.as_object().unwrap().clone()
    } else {
        serde_json::Map::new()
    };

    let mut changed = false;
    for (key, value) in pebble_hooks.as_object().unwrap() {
        if existing_hooks.get(key) != Some(value) {
            existing_hooks.insert(key.clone(), value.clone());
            changed = true;
        }
    }

    if let Some(sl) = settings.get("statusLine") {
        let is_pebble = match sl {
            serde_json::Value::String(s) => s.contains("pebble-bridge-statusline.sh"),
            serde_json::Value::Object(obj) => obj.get("command")
                .and_then(|c| c.as_str())
                .map(|s| s.contains("pebble-bridge-statusline.sh"))
                .unwrap_or(false),
            _ => false,
        };
        if is_pebble {
            settings.as_object_mut().unwrap().remove("statusLine");
            changed = true;
        }
    }

    if changed {
        settings["hooks"] = serde_json::Value::Object(existing_hooks);
        let _ = fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap());
    }
}
