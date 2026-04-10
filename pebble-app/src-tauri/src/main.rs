// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::State;
use tauri_plugin_notification::NotificationExt;

const HOOK_PORT: u16 = 9876;
const EXECUTING_TIMEOUT_SECS: u64 = 8;

#[derive(Serialize, Clone, Debug)]
struct Instance {
    id: String,
    pid: u32,
    status: String,
    working_directory: String,
    terminal_app: String,
    last_activity: u64,
}

#[derive(Deserialize, Debug)]
struct HookEvent {
    event: String,
    cwd: String,
    timestamp: u64,
}

struct AppState {
    instances: Arc<Mutex<HashMap<String, Instance>>>,
}

#[tauri::command]
fn get_instances(state: State<'_, AppState>) -> Vec<Instance> {
    let map = state.instances.lock().unwrap();
    let mut list: Vec<Instance> = map
        .values()
        .filter(|i| i.pid != 0)
        .cloned()
        .collect();
    list.sort_by(|a, b| a.working_directory.cmp(&b.working_directory));
    list
}

#[tauri::command]
fn jump_to_terminal(instance_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let map = state.instances.lock().unwrap();
    let instance = map
        .values()
        .find(|i| i.id == instance_id)
        .cloned()
        .ok_or("Instance not found")?;

    if instance.terminal_app == "iTerm2" {
        if let Some(tty) = get_process_tty(instance.pid) {
            activate_iterm2_session(&tty).map_err(|e| e.to_string())?;
        } else {
            activate_iterm2().map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

fn get_process_tty(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let output = Command::new("ps")
        .args(&["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tty.is_empty() || tty == "??" {
        None
    } else {
        Some(tty)
    }
}

fn activate_iterm2() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        tell application "iTerm2"
            activate
        end tell
    "#;

    Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()?;

    Ok(())
}

fn activate_iterm2_session(tty: &str) -> Result<(), Box<dyn std::error::Error>> {
    let script = format!(r#"
        tell application "iTerm2"
            repeat with aWindow in windows
                repeat with aTab in tabs of aWindow
                    repeat with aSession in sessions of aTab
                        if tty of aSession contains "{}" then
                            tell aWindow
                                select
                            end tell
                            tell aTab
                                select
                            end tell
                            tell aSession
                                select
                            end tell
                            return
                        end if
                    end repeat
                end repeat
            end repeat
            activate
        end tell
    "#, tty);

    Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()?;

    Ok(())
}

fn discover_instances() -> HashMap<String, Instance> {
    let mut map = HashMap::new();

    let output = Command::new("ps")
        .args(&["-eo", "pid,ppid,comm,args"])
        .output();

    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("grep") || line.contains("Pebble") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                continue;
            }
            let comm = parts[2];
            let args = parts[3..].join(" ");
            let is_claude_main = comm == "claude" || comm == "claude-code";
            let is_node_claude = comm == "node" && args.contains("claude-code");
            if !is_claude_main && !is_node_claude {
                continue;
            }
            let pid = parts[0].parse::<u32>().unwrap_or(0);
            if pid == 0 {
                continue;
            }
            let cwd = get_process_cwd(pid).unwrap_or_else(|| "Unknown".to_string());
            let terminal = detect_terminal_app(pid, &stdout);
            let id = format!("cc-{}", pid);

            map.insert(
                id.clone(),
                Instance {
                    id,
                    pid,
                    status: "waiting".to_string(),
                    working_directory: cwd,
                    terminal_app: terminal,
                    last_activity: 0,
                },
            );
        }
    }

    map
}

fn get_process_cwd(pid: u32) -> Option<String> {
    let output = Command::new("lsof")
        .args(&["-a", "-d", "cwd", "-p", &pid.to_string(), "-Fn"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with('n') {
            return Some(line[1..].to_string());
        }
    }

    None
}

fn detect_terminal_app(pid: u32, ps_output: &str) -> String {
    let mut current_pid = pid;
    for _ in 0..10 {
        let line = ps_output.lines().find(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            p.len() >= 3 && p[0].parse::<u32>().ok() == Some(current_pid)
        });
        if let Some(line) = line {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let comm = parts[2].to_lowercase();
            let args = if parts.len() > 3 { parts[3..].join(" ").to_lowercase() } else { String::new() };
            let full = format!("{} {}", comm, args);
            if full.contains("iterm2") || full.contains("iterm") {
                return "iTerm2".to_string();
            }
            if full.contains("terminal") || full.contains("apple_terminal") {
                return "Terminal.app".to_string();
            }
            if full.contains("tmux") {
                return "tmux".to_string();
            }
            if let Ok(ppid) = parts[1].parse::<u32>() {
                if ppid == current_pid || ppid == 1 || ppid == 0 {
                    break;
                }
                current_pid = ppid;
                continue;
            }
        }
        break;
    }
    "Unknown".to_string()
}

fn handle_http_request(mut stream: TcpStream, instances: Arc<Mutex<HashMap<String, Instance>>>) {
    let mut buf = [0u8; 4096];
    if let Ok(n) = stream.read(&mut buf) {
        let req = String::from_utf8_lossy(&buf[..n]);
        let first_line = req.lines().next().unwrap_or("");

        if first_line.starts_with("GET /instances") {
            let map = instances.lock().unwrap();
            let list: Vec<Instance> = map.values().cloned().collect();
            let body = serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        } else if first_line.starts_with("POST /hook") {
            if let Some(body_start) = req.find("\r\n\r\n") {
                let body = &req[body_start + 4..];
                if let Ok(event) = serde_json::from_str::<HookEvent>(body) {
                    update_instance_from_hook(instances, &event);
                }
            }
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK");
        } else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        }
    }
}

fn start_hook_server(instances: Arc<Mutex<HashMap<String, Instance>>>) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", HOOK_PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind hook server: {}", e);
                return;
            }
        };
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let inst = instances.clone();
                thread::spawn(move || handle_http_request(stream, inst));
            }
        }
    });
}

fn update_instance_from_hook(
    instances: Arc<Mutex<HashMap<String, Instance>>>,
    event: &HookEvent,
) {
    let mut map = instances.lock().unwrap();

    // Find instance by working directory
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let matched = map
        .values_mut()
        .find(|i| i.working_directory == event.cwd);

    if let Some(instance) = matched {
        let new_status = match event.event.as_str() {
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                "executing"
            }
            _ => "waiting",
        };
        instance.status = new_status.to_string();
        instance.last_activity = now_secs;
    } else {
        // Instance not yet discovered, inject it
        let id = format!("cc-{}", event.timestamp);
        let status = match event.event.as_str() {
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                "executing"
            }
            _ => "waiting",
        };
        map.insert(
            id.clone(),
            Instance {
                id,
                pid: 0,
                status: status.to_string(),
                working_directory: event.cwd.clone(),
                terminal_app: "Unknown".to_string(),
                last_activity: now_secs,
            },
        );
    }
}

fn start_state_monitor(
    instances: Arc<Mutex<HashMap<String, Instance>>>,
    app_handle: tauri::AppHandle,
) {
    let mut notified_map: HashMap<String, bool> = HashMap::new();

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));

            let mut map = instances.lock().unwrap();
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            // Rediscover processes and merge
            let discovered = discover_instances();
            let mut new_map = HashMap::new();

            for (id, disc) in discovered {
                let mut merged = disc;
                if let Some(existing) = map.get(&id) {
                    merged.status = existing.status.clone();
                    merged.last_activity = existing.last_activity;
                }
                new_map.insert(id.clone(), merged);
                notified_map.remove(&id);
            }

            // Carry over hook-injected instances that are not yet discovered by ps
            for (id, inst) in map.iter() {
                if !new_map.contains_key(id) && inst.pid == 0 {
                    // Keep only if recently active (within 60s)
                    if inst.last_activity > 0 && now_secs - inst.last_activity < 60 {
                        new_map.insert(id.clone(), inst.clone());
                    }
                }
            }

            // Timeout executing -> waiting + notify
            for (id, inst) in new_map.iter_mut() {
                if inst.status == "executing" {
                    if inst.last_activity > 0 && now_secs - inst.last_activity > EXECUTING_TIMEOUT_SECS {
                        inst.status = "waiting".to_string();
                        if !notified_map.get(id).copied().unwrap_or(false) {
                            notified_map.insert(id.clone(), true);
                            let _ = app_handle
                                .notification()
                                .builder()
                                .title("Pebble")
                                .body(format!(
                                    "Claude Code completed in {}",
                                    inst.working_directory
                                ))
                                .show();
                        }
                    }
                }
            }

            *map = new_map;
            drop(map);
        }
    });
}

fn ensure_hook_script() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let hooks_dir = home.join(".claude").join("hooks");
    let script_path = hooks_dir.join("pebble-bridge.mjs");

    let script_content = r#"#!/usr/bin/env node
import http from "http";
const eventType = process.argv[2] || "unknown";
const cwd = process.cwd();
const timestamp = Date.now();
const payload = JSON.stringify({ event: eventType, cwd, timestamp });
const req = http.request({
  hostname: "127.0.0.1",
  port: 9876,
  path: "/hook",
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(payload),
  },
  timeout: 500,
}, () => process.exit(0));
req.on("error", () => process.exit(0));
req.on("timeout", () => { req.destroy(); process.exit(0); });
req.write(payload);
req.end();
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

fn ensure_claude_hooks_config(script_path: &std::path::Path) {
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
        "PostToolUse": [{ "hooks": [{ "type": "command", "command": format!("{} PostToolUse", command_str) }] }],
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

    if changed {
        settings["hooks"] = serde_json::Value::Object(existing_hooks);
        let _ = fs::write(&settings_path, serde_json::to_string_pretty(&settings).unwrap());
    }
}

fn main() {
    let instances = Arc::new(Mutex::new(HashMap::new()));

    let script_path = ensure_hook_script();
    ensure_claude_hooks_config(&script_path);
    start_hook_server(instances.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            instances: instances.clone(),
        })
        .setup(move |app| {
            start_state_monitor(instances.clone(), app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_instances, jump_to_terminal])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
