use std::fs;
use std::path::PathBuf;

pub fn ensure_hook_script() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let hooks_dir = home.join(".claude").join("hooks");
    let script_path = hooks_dir.join("pebble-bridge.mjs");

    let script_content = r#"#!/usr/bin/env node
import http from "http";
import { execSync } from "child_process";

const eventType = process.argv[2] || "unknown";
const cwd = process.cwd();
const timestamp = Date.now();

function findClaudePid(startPid) {
  let pid = startPid;
  while (pid > 1) {
    try {
      const comm = execSync(`ps -p ${pid} -o comm=`, { encoding: "utf8" }).trim();
      if (comm === "claude" || comm === "claude-code") {
        return pid;
      }
      const ppid = parseInt(execSync(`ps -p ${pid} -o ppid=`, { encoding: "utf8" }).trim(), 10);
      if (ppid === pid || ppid <= 0) break;
      pid = ppid;
    } catch (e) {
      break;
    }
  }
  return null;
}

const senderPid = findClaudePid(process.ppid);

let stdinData = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => { stdinData += chunk; });
process.stdin.on("end", () => {
  let body = { event: eventType, cwd, timestamp };
  if (senderPid) {
    body.sender_pid = senderPid;
  }
  if (stdinData.trim()) {
    try {
      const parsed = JSON.parse(stdinData);
      body = { ...parsed, ...body };
    } catch (e) {
      body.stdin = stdinData;
    }
  }
  const payload = JSON.stringify(body);
  const req = http.request({
    hostname: "127.0.0.1",
    port: 9876,
    path: "/hook",
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Content-Length": Buffer.byteLength(payload),
    },
    timeout: eventType === "PermissionRequest" ? 300000 : 500,
  }, (res) => {
    let responseData = "";
    res.setEncoding("utf8");
    res.on("data", chunk => { responseData += chunk; });
    res.on("end", () => {
      if (responseData.trim()) {
        console.log(responseData);
      }
      process.exit(0);
    });
  });
  req.on("error", () => process.exit(0));
  req.on("timeout", () => { req.destroy(); process.exit(0); });
  req.write(payload);
  req.end();
});
"#;

    if let Ok(existing) = fs::read_to_string(&script_path) {
        if existing.trim() == script_content.trim() {
            return script_path;
        }
    }

    let _ = fs::create_dir_all(&hooks_dir);
    let _ = fs::write(&script_path, script_content);
    script_path
}

pub fn ensure_claude_hooks_config(script_path: &std::path::Path) {
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

    let command_str = format!("node {}", script_path.to_string_lossy());

    let pebble_hooks = serde_json::json!({
        "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": format!("{} UserPromptSubmit", command_str) }] }],
        "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PreToolUse", command_str) }] }],
        "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUse", command_str) }] }],
        "PostToolUseFailure": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PostToolUseFailure", command_str) }] }],
        "PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": format!("{} PermissionRequest", command_str), "timeout": 300 }] }],
        "Stop": [{ "hooks": [{ "type": "command", "command": format!("{} Stop", command_str) }] }]
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

    // Remove old Pebble statusLine if it exists (migration from v0.1.x)
    if let Some(sl) = settings.get("statusLine") {
        let is_pebble = match sl {
            serde_json::Value::String(cmd) => cmd.contains("pebble-bridge-statusline.sh"),
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

pub fn uninstall_hooks(script_path: &std::path::Path) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let hooks_dir = home.join(".claude").join("hooks");
    let _ = fs::remove_file(hooks_dir.join("pebble-bridge-statusline.sh"));
    let _ = fs::remove_file(script_path);
}
