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
use tauri::{Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;
#[cfg(target_os = "macos")]
use cocoa::appkit::{NSWindow, NSWindowCollectionBehavior};
#[cfg(target_os = "macos")]
use cocoa::base::id;
#[cfg(target_os = "macos")]
use cocoa::foundation::{NSPoint, NSRect, NSSize};

const HOOK_PORT: u16 = 9876;
const EXECUTING_TIMEOUT_SECS: u64 = 30;

#[derive(Serialize, Clone, Debug)]
struct PendingPermission {
    tool_name: String,
    tool_use_id: String,
    prompt: String,
    choices: Vec<String>,
    default_choice: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
struct Instance {
    id: String,
    pid: u32,
    status: String,
    working_directory: String,
    terminal_app: String,
    last_activity: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_permission: Option<PendingPermission>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_hook_event: Option<HookEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct HookEvent {
    event: String,
    cwd: String,
    timestamp: u64,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default, rename = "tool_input")]
    tool_input: Option<serde_json::Value>,
    #[serde(default, rename = "permission_mode")]
    permission_mode: Option<String>,
    #[serde(default, rename = "tool_use_id")]
    tool_use_id: Option<String>,
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

#[tauri::command]
fn respond_permission(
    instance_id: String,
    choice: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let instance = {
        let map = state.instances.lock().unwrap();
        map.values()
            .find(|i| i.id == instance_id)
            .cloned()
            .ok_or("Instance not found")?
    };

    let perm = instance
        .pending_permission
        .ok_or("No pending permission for this instance")?;

    let target_idx = perm
        .choices
        .iter()
        .position(|c| c == &choice)
        .ok_or("Invalid choice")?;
    let default_idx = perm
        .default_choice
        .as_ref()
        .and_then(|d| perm.choices.iter().position(|c| c == d))
        .unwrap_or(0);

    if instance.terminal_app == "iTerm2" {
        if let Some(tty) = get_process_tty(instance.pid) {
            inject_permission_response_to_iterm2(&tty, target_idx, default_idx)
                .map_err(|e| e.to_string())?;
        } else {
            return Err("TTY not found".to_string());
        }
    } else {
        return Err("Permission response only supported for iTerm2".to_string());
    }

    let mut map = state.instances.lock().unwrap();
    if let Some(inst) = map.values_mut().find(|i| i.id == instance_id) {
        inst.status = "executing".to_string();
        inst.pending_permission = None;
    }

    Ok(())
}

#[tauri::command]
fn resize_window_centered(
    width: f64,
    height: f64,
    animate: bool,
    window: tauri::Window,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        unsafe {
            if let Ok(raw) = window.ns_window() {
                let ns_window: id = raw as id;
                let screen_cls = class!(NSScreen);
                let screen: id = msg_send![screen_cls, mainScreen];
                let frame: NSRect = msg_send![screen, frame];
                let x = frame.size.width / 2.0 - width / 2.0;
                let y = frame.size.height - height;
                let origin = NSPoint::new(x, y);
                let new_frame = NSRect::new(origin, NSSize::new(width, height));
                if animate {
                    let () = msg_send![ns_window, setFrame:new_frame display:true animate:true];
                } else {
                    let () = msg_send![ns_window, setFrame:new_frame display:true];
                }
                return Ok(());
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn get_instance_preview(instance_id: String, state: State<'_, AppState>) -> Result<String, String> {
    let map = state.instances.lock().unwrap();
    let instance = map
        .values()
        .find(|i| i.id == instance_id)
        .cloned()
        .ok_or("Instance not found")?;

    // Primary: use hook event if available
    if let Some(ref hook) = instance.last_hook_event {
        let preview = format_hook_preview(hook);
        if !preview.is_empty() {
            return Ok(preview);
        }
    }

    // Fallback: read from iTerm2 if applicable
    if instance.terminal_app == "iTerm2" {
        if let Some(tty) = get_process_tty(instance.pid) {
            let lines = read_iterm2_last_lines(&tty, 3);
            let filtered: Vec<String> = lines
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| {
                    !s.is_empty()
                        && !s.starts_with('$')
                        && !s.starts_with('#')
                        && !s.starts_with('>')
                        && !s.starts_with('%')
                        && !s.starts_with("$")
                        && !s.starts_with("❯")
                        && !s.starts_with("●")
                })
                .collect();
            if let Some(last) = filtered.last() {
                return Ok(last.clone());
            }
        }
    }

    Ok("No recent activity".to_string())
}

fn format_hook_preview(hook: &HookEvent) -> String {
    match hook.event.as_str() {
        "UserPromptSubmit" => {
            if let Some(ref input) = hook.tool_input {
                let text = input.to_string();
                let truncated = if text.len() > 80 {
                    format!("{}...", &text[..80])
                } else {
                    text
                };
                format!("You: {}", truncated)
            } else {
                "You: ...".to_string()
            }
        }
        "PreToolUse" => {
            let tool = hook.tool_name.as_deref().unwrap_or("Tool");
            format!("Using {}", tool)
        }
        "PostToolUse" => {
            let tool = hook.tool_name.as_deref().unwrap_or("Tool");
            format!("{} completed", tool)
        }
        "PostToolUseFailure" => {
            let tool = hook.tool_name.as_deref().unwrap_or("Tool");
            format!("{} failed", tool)
        }
        "Stop" => "Stopped".to_string(),
        _ => "".to_string(),
    }
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
    let script = format!(
        r#"
        tell application "iTerm2"
            activate
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
        end tell
    "#,
        tty
    );

    Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()?;

    Ok(())
}

fn inject_permission_response_to_iterm2(
    tty: &str,
    target_idx: usize,
    default_idx: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut key_lines = Vec::new();
    if target_idx > default_idx {
        for _ in 0..(target_idx - default_idx) {
            key_lines.push(r#"                                write text (ASCII character 27) & "[B" newline NO"#);
        }
    } else if target_idx < default_idx {
        for _ in 0..(default_idx - target_idx) {
            key_lines.push(r#"                                write text (ASCII character 27) & "[A" newline NO"#);
        }
    }
    key_lines.push(r#"                                write text (ASCII character 13) newline NO"#);
    let keys_body = key_lines.join("\n");

    let script = format!(
        r#"
        tell application "iTerm2"
            repeat with aWindow in windows
                repeat with aTab in tabs of aWindow
                    repeat with aSession in sessions of aTab
                        if tty of aSession contains "{}" then
                            tell aSession
{}
                            end tell
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
    "#,
        tty, keys_body
    );

    Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()?;

    Ok(())
}

fn read_iterm2_last_lines(tty: &str, n: usize) -> Vec<String> {
    let n_lines = n.max(1);
    let script = format!(
        r#"
        tell application "iTerm2"
            repeat with aWindow in windows
                repeat with aTab in tabs of aWindow
                    repeat with aSession in sessions of aTab
                        if tty of aSession contains "{}" then
                            set sessionContents to contents of aSession
                            set allLines to paragraphs of sessionContents
                            set totalLines to count of allLines
                            set startLine to totalLines - {}
                            if startLine < 1 then set startLine to 1
                            set resultLines to {{}}
                            repeat with i from startLine to totalLines
                                set end of resultLines to item i of allLines
                            end repeat
                            set AppleScript's text item delimiters to linefeed
                            return resultLines as string
                        end if
                    end repeat
                end repeat
            end repeat
            return ""
        end tell
    "#,
        tty, n_lines
    );

    match Command::new("osascript").arg("-e").arg(&script).output() {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines().map(|s| s.to_string()).collect()
        }
        Err(_) => Vec::new(),
    }
}

fn parse_permission_choices(lines: &[String]) -> Option<PendingPermission> {
    let mut prompt_idx = None;
    for (i, line) in lines.iter().enumerate().rev() {
        let trimmed = line.trim();
        if trimmed.ends_with('?') || trimmed.ends_with(':') {
            if trimmed.len() > 5 {
                prompt_idx = Some(i);
                break;
            }
        }
    }
    let prompt_idx = prompt_idx?;

    let prompt = lines[prompt_idx].trim().to_string();
    let mut choices: Vec<String> = Vec::new();
    let mut default_choice: Option<String> = None;

    for line in lines.iter().skip(prompt_idx + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let is_selected = trimmed.starts_with("> ")
            || trimmed.starts_with("❯ ")
            || trimmed.starts_with("● ")
            || trimmed.starts_with("* ");

        let clean = if trimmed.starts_with("> ") {
            &trimmed[2..]
        } else if trimmed.starts_with("❯ ") {
            &trimmed["❯ ".len()..]
        } else if trimmed.starts_with("● ") {
            &trimmed["● ".len()..]
        } else if trimmed.starts_with("* ") {
            &trimmed[2..]
        } else if trimmed.starts_with("- ") {
            &trimmed[2..]
        } else if trimmed.starts_with("○ ") || trimmed.starts_with("◯ ") {
            &trimmed["○ ".len()..]
        } else if trimmed.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && trimmed.chars().nth(1) == Some('.')
        {
            trimmed[2..].trim_start()
        } else {
            if choices.is_empty() {
                continue;
            } else {
                break;
            }
        };

        let clean = clean.trim().to_string();
        if !clean.is_empty() {
            if is_selected && default_choice.is_none() {
                default_choice = Some(clean.clone());
            }
            choices.push(clean);
        }
    }

    if choices.is_empty() {
        None
    } else {
        Some(PendingPermission {
            tool_name: "Claude".to_string(),
            tool_use_id: "".to_string(),
            prompt,
            choices,
            default_choice,
        })
    }
}

fn default_choices_for_tool(tool_name: &str) -> Vec<String> {
    match tool_name {
        "Bash" => vec![
            "Yes".to_string(),
            "No".to_string(),
            "Always allow Bash".to_string(),
        ],
        "Edit" => vec![
            "Yes".to_string(),
            "No".to_string(),
            "Always allow Edit".to_string(),
        ],
        "Write" => vec![
            "Yes".to_string(),
            "No".to_string(),
            "Always allow Write".to_string(),
        ],
        "Read" => vec![
            "Yes".to_string(),
            "No".to_string(),
            "Always allow Read".to_string(),
        ],
        _ => vec!["Yes".to_string(), "No".to_string()],
    }
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
                    pending_permission: None,
                    last_hook_event: None,
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
            let args = if parts.len() > 3 {
                parts[3..].join(" ").to_lowercase()
            } else {
                String::new()
            };
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
    let mut buf = [0u8; 65536];
    if let Ok(n) = stream.read(&mut buf) {
        let req = String::from_utf8_lossy(&buf[..n]);
        let first_line = req.lines().next().unwrap_or("");

        if first_line.starts_with("GET /instances") {
            let map = instances.lock().unwrap();
            let mut list: Vec<Instance> = map.values().cloned().collect();
            drop(map);
            list.sort_by(|a, b| a.working_directory.cmp(&b.working_directory));
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
    let instances_for_scrape = instances.clone();
    let mut map = instances.lock().unwrap();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let is_permission_event = event.event == "PreToolUse"
        && !matches!(
            event.permission_mode.as_deref(),
            Some("bypassPermissions" | "dontAsk" | "auto" | "acceptEdits")
        );

    let matched = map.values_mut().find(|i| i.working_directory == event.cwd);

    if let Some(instance) = matched {
        let new_status = match event.event.as_str() {
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                "executing"
            }
            _ => "waiting",
        };
        instance.status = new_status.to_string();
        instance.last_activity = now_secs;
        instance.last_hook_event = Some(event.clone());

        if is_permission_event {
            let tool_name = event.tool_name.clone().unwrap_or_else(|| "Tool".to_string());
            let tool_use_id = event.tool_use_id.clone().unwrap_or_default();
            let tty = get_process_tty(instance.pid);
            let id = instance.id.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(500));
                let lines = tty.as_ref().map(|t| read_iterm2_last_lines(t, 30)).unwrap_or_default();
                if let Some(parsed) = parse_permission_choices(&lines) {
                    let mut map = instances_for_scrape.lock().unwrap();
                    if let Some(inst) = map.get_mut(&id) {
                        inst.status = "needs_permission".to_string();
                        inst.pending_permission = Some(PendingPermission {
                            tool_name,
                            tool_use_id,
                            prompt: parsed.prompt,
                            choices: parsed.choices,
                            default_choice: parsed.default_choice,
                        });
                    }
                }
            });
        } else {
            instance.pending_permission = None;
        }
    } else {
        let id = format!("cc-{}", event.timestamp);
        let status = if is_permission_event {
            "needs_permission"
        } else {
            match event.event.as_str() {
                "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                    "executing"
                }
                _ => "waiting",
            }
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
                pending_permission: None,
                last_hook_event: Some(event.clone()),
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

            let discovered = discover_instances();
            let mut new_map = HashMap::new();

            for (id, disc) in discovered {
                let mut merged = disc;
                if let Some(existing) = map.get(&id) {
                    merged.status = existing.status.clone();
                    merged.last_activity = existing.last_activity;
                    merged.pending_permission = existing.pending_permission.clone();
                    merged.last_hook_event = existing.last_hook_event.clone();
                }
                new_map.insert(id.clone(), merged);
                notified_map.remove(&id);
            }

            for (id, inst) in map.iter() {
                if !new_map.contains_key(id) && inst.pid == 0 {
                    if inst.last_activity > 0 && now_secs - inst.last_activity < 60 {
                        new_map.insert(id.clone(), inst.clone());
                    }
                }
            }

            for (id, inst) in new_map.iter_mut() {
                if inst.status == "executing" {
                    if inst.last_activity > 0
                        && now_secs - inst.last_activity > EXECUTING_TIMEOUT_SECS
                    {
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
            let mut list: Vec<Instance> = map.values().cloned().collect();
            drop(map);
            list.sort_by(|a, b| a.working_directory.cmp(&b.working_directory));
            let _ = app_handle.emit("instances-updated", list);
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

let stdinData = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => { stdinData += chunk; });
process.stdin.on("end", () => {
  let body = { event: eventType, cwd, timestamp };
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
    timeout: 500,
  }, () => process.exit(0));
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
        "PreToolUse": [{ "hooks": [{ "type": "command", "command": format!("{} PreToolUse", command_str) }] }],
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

#[cfg(target_os = "macos")]
unsafe fn setup_notch_overlay(window: &tauri::WebviewWindow) {
    if let Ok(raw) = window.ns_window() {
        let ns_window: id = raw as id;
        // Above menu bar level so macOS does not push us out of the notch area
        ns_window.setLevel_(25);
        ns_window.setCollectionBehavior_(
            NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
                | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle,
        );
        ns_window.setHasShadow_(false);
        ns_window.setHidesOnDeactivate_(false);
        ns_window.setMovable_(false);
        ns_window.setMovableByWindowBackground_(false);

        // Position window top-center exactly
        let screen_cls = class!(NSScreen);
        let screen: id = msg_send![screen_cls, mainScreen];
        let frame: NSRect = msg_send![screen, frame];
        let win_size = ns_window.frame().size;
        let x = frame.size.width / 2.0 - win_size.width / 2.0;
        let y = frame.size.height;
        let origin = NSPoint::new(x, y);
        let () = msg_send![ns_window, setFrameTopLeftPoint: origin];

        // Ensure transparent background so CSS clip-path corners show through
        let color_cls = class!(NSColor);
        let clear: id = msg_send![color_cls, clearColor];
        let () = msg_send![ns_window, setBackgroundColor: clear];
        let () = msg_send![ns_window, setOpaque: false];
    }
}

#[cfg(target_os = "macos")]
unsafe fn start_hover_tracker(window: tauri::WebviewWindow) {
    thread::spawn(move || {
        let mut was_inside = false;
        loop {
            thread::sleep(Duration::from_millis(80));
            if let Ok(raw) = window.ns_window() {
                let ns_window: id = raw as id;
                let frame: NSRect = ns_window.frame();
                let mouse: NSPoint = {
                    let ev_cls = class!(NSEvent);
                    let pt: NSPoint = msg_send![ev_cls, mouseLocation];
                    pt
                };
                let inside =
                    mouse.x >= frame.origin.x
                        && mouse.x <= frame.origin.x + frame.size.width
                        && mouse.y >= frame.origin.y
                        && mouse.y <= frame.origin.y + frame.size.height;
                if inside != was_inside {
                    was_inside = inside;
                    let _ = window.emit("pebble-hover", inside);
                }
            }
        }
    });
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
            #[cfg(target_os = "macos")]
            {
                if let Some(window) = app.handle().get_webview_window("main") {
                    let _ = window.set_resizable(false);
                    let _ = window.set_size(tauri::Size::Logical(
                        tauri::LogicalSize { width: 324.0, height: 50.0 }
                    ));
                    unsafe {
                        setup_notch_overlay(&window);
                        start_hover_tracker(window.clone());
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                if let Some(window) = app.handle().get_webview_window("main") {
                    if let Ok(Some(monitor)) = window.current_monitor() {
                        let size = monitor.size();
                        let scale = monitor.scale_factor();
                        let logical_width = size.width as f64 / scale;
                        let w = 300.0;
                        let x = (logical_width - w) / 2.0;
                        let _ = window.set_position(tauri::Position::Logical(
                            tauri::LogicalPosition { x, y: 0.0 }
                        ));
                        let _ = window.set_size(tauri::Size::Logical(
                            tauri::LogicalSize { width: w, height: 52.0 }
                        ));
                    }
                }
            }
            start_state_monitor(instances.clone(), app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_instances, jump_to_terminal, respond_permission, get_instance_preview, resize_window_centered])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
