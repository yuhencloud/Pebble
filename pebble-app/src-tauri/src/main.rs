// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]


mod types;
mod platform;
mod transcript;
mod hook;
use types::{AppState, HookEvent, Instance, PendingPermission, SubagentInfo};
use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use parking_lot::Mutex;
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

const EXECUTING_TIMEOUT_SECS: u64 = 30;

fn is_related_cwd(a: &str, b: &str) -> bool {
    let a = std::path::Path::new(a);
    let b = std::path::Path::new(b);
    a == b || a.starts_with(b) || b.starts_with(a)
}

fn build_grouped_instances(map: &HashMap<String, Instance>) -> Vec<Instance> {
    let mut result: Vec<Instance> = map.values()
        .filter(|i| i.pid != 0 || (i.last_activity > 0 && i.last_hook_event.is_some()))
        .cloned()
        .collect();
    result.sort_by(|a, b| a.working_directory.cmp(&b.working_directory)
        .then_with(|| b.last_activity.cmp(&a.last_activity)));
    result
}

#[tauri::command]
fn get_instances(state: State<'_, AppState>) -> Vec<Instance> {
    let map = state.instances.lock();
    build_grouped_instances(&map)
}

#[tauri::command]
fn jump_to_terminal(instance_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let map = state.instances.lock();
    let instance = map
        .values()
        .find(|i| i.id == instance_id)
        .cloned()
        .ok_or("Instance not found")?;
    platform::jump::jump_to_terminal(instance.pid, &instance.terminal_app)
}

#[tauri::command]
fn respond_permission(
    instance_id: String,
    choice: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let instance = {
        let map = state.instances.lock();
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
        if let Some(tty) = platform::jump::get_process_tty(instance.pid) {
            inject_permission_response_to_iterm2(&tty, target_idx, default_idx)
                .map_err(|e| e.to_string())?;
        } else {
            return Err("TTY not found".to_string());
        }
    } else {
        return Err("Permission response only supported for iTerm2".to_string());
    }

    let mut map = state.instances.lock();
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
                let screen: id = msg_send![ns_window, screen];
                if screen.is_null() {
                    return Err("Window has no screen".to_string());
                }
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
    let map = state.instances.lock();
    let instance = map
        .values()
        .find(|i| i.id == instance_id)
        .cloned()
        .ok_or("Instance not found")?;

    // Primary: use conversation log if available
    if !instance.conversation_log.is_empty() {
        let start = instance.conversation_log.len().saturating_sub(3);
        let lines = instance.conversation_log[start..].join("\n");
        return Ok(lines);
    }

    // Fallback: read from iTerm2 if applicable
    if instance.terminal_app == "iTerm2" {
        if let Some(tty) = platform::jump::get_process_tty(instance.pid) {
            let lines = read_iterm2_last_lines(&tty, 8);
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

            if filtered.len() >= 2 {
                let start = filtered.len().saturating_sub(3);
                return Ok(filtered[start..].join("\n"));
            } else if let Some(last) = filtered.last() {
                return Ok(last.clone());
            }
        }
    }

    Ok("No recent activity".to_string())
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

    if choices.len() < 2 {
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

fn discover_instances() -> HashMap<String, Instance> {
    let mut map = HashMap::new();

    let all = platform::discovery::list_processes();
    let claudes = platform::discovery::find_claude_processes();

    let mut claude_pids = std::collections::HashSet::new();
    for p in &all {
        let is_claude_main = p.comm == "claude" || p.comm == "claude-code";
        let is_node_claude = p.comm == "node" && p.args.contains("claude-code");
        if is_claude_main || is_node_claude {
            claude_pids.insert(p.pid);
        }
    }

    let mut children: HashMap<u32, Vec<(u32, String, String)>> = HashMap::new();
    for p in &all {
        if claude_pids.contains(&p.pid) && claude_pids.contains(&p.ppid) && p.pid != p.ppid {
            children.entry(p.ppid).or_default().push((p.pid, p.comm.clone(), p.args.clone()));
        }
    }

    fn collect_subagents(
        pid: u32,
        children: &HashMap<u32, Vec<(u32, String, String)>>,
        depth: usize,
    ) -> Vec<SubagentInfo> {
        if depth >= 5 {
            return Vec::new();
        }
        let mut result = Vec::new();
        if let Some(kids) = children.get(&pid) {
            for (cid, comm, args) in kids {
                let name = args.split_whitespace().next().unwrap_or(comm).to_string();
                result.push(SubagentInfo {
                    id: format!("cc-{}", cid),
                    status: "executing".to_string(),
                    name,
                });
                result.extend(collect_subagents(*cid, children, depth + 1));
            }
        }
        result
    }

    let ps_output = Command::new("ps")
        .args(&["-eo", "pid,ppid,comm,args"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    for p in claudes {
        let cwd = platform::cwd::get_process_cwd(p.pid).unwrap_or_else(|| "Unknown".to_string());
        let terminal = platform::terminal::detect_terminal_app(p.pid, &ps_output);
        let id = format!("cc-{}", p.pid);
        let subagents = collect_subagents(p.pid, &children, 0);

        map.insert(
            id.clone(),
            Instance {
                id,
                pid: p.pid,
                status: "waiting".to_string(),
                working_directory: cwd,
                terminal_app: terminal,
                last_activity: 0,
                pending_permission: None,
                last_hook_event: None,
                subagents,
                model: None,
                permission_mode: None,
                context_percent: None,
                conversation_log: Vec::new(),
                session_start: None,
                transcript_path: None,
                session_name: None,
            },
        );
    }

    map
}
fn extract_model_string(val: &Option<serde_json::Value>) -> Option<String> {
    val.as_ref().and_then(|v| {
        if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else if let Some(obj) = v.as_object() {
            obj.get("display_name")
                .and_then(|n| n.as_str().map(|s| s.to_string()))
                .or_else(|| obj.get("id").and_then(|n| n.as_str().map(|s| s.to_string())))
        } else {
            None
        }
    })
}

fn extract_context_percent_from_payload(
    ctx: &Option<serde_json::Value>,
    explicit: Option<u8>,
) -> Option<u8> {
    explicit.or_else(|| {
        ctx.as_ref().and_then(|v| {
            v.as_object()
                .and_then(|o| o.get("used_percentage"))
                .and_then(|p| p.as_f64().map(|n| n.round() as u8))
        })
    })
}

fn update_instance_from_hook(
    instances: Arc<Mutex<HashMap<String, Instance>>>,
    event: &HookEvent,
    transcript_path: Option<String>,
    sender_pid: Option<u32>,
) {
    // Pre-read transcript data before acquiring lock
    let transcript_data = if event.event == "StatusLine" {
        transcript_path.as_ref().map(|path| {
            let session_start = transcript::read_session_start_from_transcript(path);
            let preview = transcript::read_transcript_preview(path, 3);
            (session_start, preview)
        })
    } else {
        None
    };

    let instances_for_scrape = instances.clone();
    let mut map = instances.lock();

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let is_statusline = event.event == "StatusLine";
    let is_permission_event = event.event == "PreToolUse"
        && !matches!(
            event.permission_mode.as_deref(),
            Some("bypassPermissions" | "dontAsk" | "auto" | "acceptEdits")
        );

    let mut matched_id: Option<String> = None;
    if let Some(spid) = sender_pid {
        let candidate_id = format!("cc-{spid}");
        if let Some(inst) = map.get(&candidate_id) {
            if inst.working_directory == event.cwd || is_related_cwd(&inst.working_directory, &event.cwd) {
                matched_id = Some(candidate_id);
            }
        }
    }
    if matched_id.is_none() {
        if let Some(ref tp) = transcript_path {
            matched_id = map.values().find(|i| i.transcript_path.as_ref() == Some(tp)).map(|i| i.id.clone());
        }
    }
    if matched_id.is_none() {
        let mut max_la = 0u64;
        for i in map.values() {
            if i.working_directory == event.cwd && i.last_activity > max_la {
                max_la = i.last_activity;
                matched_id = Some(i.id.clone());
            }
        }
    }
    if matched_id.is_none() {
        let mut max_la = 0u64;
        for i in map.values() {
            if is_related_cwd(&i.working_directory, &event.cwd) && i.last_activity > max_la {
                max_la = i.last_activity;
                matched_id = Some(i.id.clone());
            }
        }
    }

    if let Some(ref id) = matched_id {
        if let Some(instance) = map.get_mut(id) {
            if is_statusline {
                instance.last_activity = now_secs;
                if let Some(ref tp) = transcript_path {
                    if !tp.is_empty() {
                        instance.transcript_path = Some(tp.clone());
                    }
                }
                if let Some(ref sn) = event.session_name {
                    if !sn.is_empty() {
                        instance.session_name = Some(sn.clone());
                    }
                }
                if let Some(ref m) = event.model {
                    if !m.is_empty() {
                        instance.model = Some(m.clone());
                    }
                }
                if let Some(cp) = event.context_percent {
                    instance.context_percent = Some(cp);
                }
                if instance.session_start.is_none() {
                    if let Some((Some(start_ts), _)) = &transcript_data {
                        instance.session_start = Some(*start_ts);
                    }
                }
                if let Some((_, preview)) = &transcript_data {
                    if !preview.is_empty() {
                        instance.conversation_log = preview.clone();
                    }
                }
                return;
            }

            let new_status = match event.event.as_str() {
                "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
                    "executing"
                }
                _ => "waiting",
            };
            instance.status = new_status.to_string();
        instance.last_activity = now_secs;
        instance.last_hook_event = Some(event.clone());
        if let Some(ref pm) = event.permission_mode {
            instance.permission_mode = Some(pm.clone());
        }

        if is_permission_event {
            let tool_name = event.tool_name.clone().unwrap_or_else(|| "Tool".to_string());
            let tool_use_id = event.tool_use_id.clone().unwrap_or_default();
            let tty = platform::jump::get_process_tty(instance.pid);
            let id = instance.id.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(500));
                let lines = tty.as_ref().map(|t| read_iterm2_last_lines(t, 30)).unwrap_or_default();
                if let Some(parsed) = parse_permission_choices(&lines) {
                    let mut map = instances_for_scrape.lock();
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
        }
    } else if !is_statusline {
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
                subagents: Vec::new(),
                model: event.model.clone(),
                permission_mode: event.permission_mode.clone(),
                context_percent: event.context_percent,
                conversation_log: Vec::new(),
                session_start: None,
                transcript_path: None,
                session_name: None,
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

            let mut map = instances.lock();
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
                    merged.subagents = existing.subagents.clone();
                    merged.model = existing.model.clone();
                    merged.permission_mode = existing.permission_mode.clone();
                    merged.context_percent = existing.context_percent;
                    merged.conversation_log = existing.conversation_log.clone();
                    merged.session_start = existing.session_start;
                    merged.transcript_path = existing.transcript_path.clone();
                    merged.session_name = existing.session_name.clone();
                }
                new_map.insert(id.clone(), merged);
                notified_map.remove(&id);
            }

            for (id, inst) in map.iter() {
                if !new_map.contains_key(id) && inst.pid == 0 {
                    let mut merged = false;
                    for disc in new_map.values_mut() {
                        if is_related_cwd(&inst.working_directory, &disc.working_directory) {
                            if inst.last_activity > disc.last_activity {
                                disc.last_activity = inst.last_activity;
                            }
                            if !inst.conversation_log.is_empty() {
                                disc.conversation_log.clone_from(&inst.conversation_log);
                            }
                            if inst.session_start.is_some() {
                                disc.session_start = inst.session_start;
                            }
                            if inst.transcript_path.is_some() {
                                disc.transcript_path.clone_from(&inst.transcript_path);
                            }
                            if inst.session_name.is_some() {
                                disc.session_name.clone_from(&inst.session_name);
                            }
                            if inst.last_hook_event.is_some() {
                                disc.last_hook_event.clone_from(&inst.last_hook_event);
                            }
                            if inst.model.is_some() {
                                disc.model.clone_from(&inst.model);
                            }
                            if inst.permission_mode.is_some() {
                                disc.permission_mode.clone_from(&inst.permission_mode);
                            }
                            if inst.context_percent.is_some() {
                                disc.context_percent = inst.context_percent;
                            }
                            merged = true;
                            break;
                        }
                    }
                    if !merged && inst.last_activity > 0 && now_secs - inst.last_activity < 60 {
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
            let list = build_grouped_instances(&map);
            drop(map);
            let _ = app_handle.emit("instances-updated", list);
        }
    });
}

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
        let screen: id = msg_send![ns_window, screen];
        if screen.is_null() { return; }
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
unsafe fn start_hover_tracker(window: tauri::WebviewWindow, running: Arc<std::sync::atomic::AtomicBool>) {
    thread::spawn(move || {
        let mut was_inside = false;
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(16));
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
    let hover_running = Arc::new(std::sync::atomic::AtomicBool::new(true));

    let script_path = hook::bridge::ensure_hook_script();
    hook::bridge::ensure_claude_hooks_config(&script_path);

    let instances_for_hook = instances.clone();
    hook::server::start_hook_server(instances.clone(), move |payload| {
        let model = extract_model_string(&payload.raw_model)
            .or_else(|| {
                payload.context_window.as_ref().and_then(|cw| {
                    cw.as_object()
                        .and_then(|o| o.get("model"))
                        .and_then(|m| extract_model_string(&Some(m.clone())))
                })
            });
        let context_percent = extract_context_percent_from_payload(
            &payload.context_window,
            payload.context_percent,
        );
        let event = HookEvent {
            event: payload.event.clone(),
            cwd: payload.cwd.clone(),
            timestamp: payload.timestamp,
            tool_name: payload.tool_name.clone(),
            tool_input: payload.tool_input.clone(),
            permission_mode: payload.permission_mode.clone(),
            tool_use_id: payload.tool_use_id.clone(),
            model,
            context_percent,
            session_name: payload.session_name.clone(),
        };
        update_instance_from_hook(instances_for_hook.clone(), &event, payload.transcript_path.clone(), payload.sender_pid);
    });

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
                    let hr = hover_running.clone();
                    window.on_window_event(move |event| {
                        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                            hr.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                    unsafe {
                        setup_notch_overlay(&window);
                        start_hover_tracker(window.clone(), hover_running.clone());
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
