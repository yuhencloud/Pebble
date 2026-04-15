// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod types;
mod platform;
mod transcript;
mod session;
mod hook;
mod adapter;

use adapter::AdapterRegistry;
use types::{AppState, Instance};
use std::collections::HashMap;
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
    // Clear stale permission when user explicitly jumps to terminal
    {
        let mut map = state.instances.lock();
        if let Some(inst) = map.values_mut().find(|i| i.id == instance_id) {
            if inst.status == "needs_permission" && inst.pending_permission.is_some() {
                inst.status = "executing".to_string();
                inst.pending_permission = None;
            }
        }
    }
    let instance = {
        let map = state.instances.lock();
        map.values()
            .find(|i| i.id == instance_id)
            .cloned()
            .ok_or("Instance not found")?
    };
    let adapter = state.registry.find_adapter_for_event(&adapter::HookPayload {
        event: "discover".to_string(),
        cwd: instance.working_directory.clone(),
        timestamp: 0,
        tool_name: None,
        tool_input: None,
        permission_mode: None,
        tool_use_id: None,
        model: None,
        context_percent: None,
        session_name: None,
        transcript_path: None,
        choices: None,
        default_choice: None,
        wezterm_pane_id: None,
        wt_session_id: None,
        wezterm_unix_socket: None,
        agent_id: None,
        agent_type: None,
        source: None,
    }).ok_or("No adapter found")?;
    adapter.jump_to_terminal(&instance)
}

#[tauri::command]
fn resize_window_centered(
    width: f64,
    height: f64,
    _animate: bool,
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
                let y = frame.origin.y + frame.size.height - height;
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
        if let Ok(Some(monitor)) = window.current_monitor() {
            let size = monitor.size();
            let scale = monitor.scale_factor();
            let logical_width = size.width as f64 / scale;
            let x = (logical_width - width) / 2.0;
            let _ = window.set_position(tauri::Position::Logical(
                tauri::LogicalPosition { x, y: 0.0 }
            ));
        }
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn bring_to_front(window: tauri::Window) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    unsafe {
        let hwnd_val = match window.hwnd() {
            Ok(h) => h.0 as isize,
            Err(e) => return Err(e.to_string()),
        };
        if hwnd_val == 0 {
            return Err("Invalid HWND".to_string());
        }
        let hwnd = windows::Win32::Foundation::HWND(hwnd_val as *mut core::ffi::c_void);
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        };
        SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
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

    let adapter = state.registry.find_adapter_for_event(&adapter::HookPayload {
        event: "discover".to_string(),
        cwd: instance.working_directory.clone(),
        timestamp: 0,
        tool_name: None,
        tool_input: None,
        permission_mode: None,
        tool_use_id: None,
        model: None,
        context_percent: None,
        session_name: None,
        transcript_path: None,
        choices: None,
        default_choice: None,
        wezterm_pane_id: None,
        wt_session_id: None,
        wezterm_unix_socket: None,
        agent_id: None,
        agent_type: None,
        source: None,
    }).ok_or("No adapter found")?;

    let states = state.adapter_states.lock();
    let adapter_state = states.get(&instance_id).cloned().unwrap_or_default();
    let preview = adapter.get_preview(&adapter_state);
    Ok(preview.join("\n"))
}

fn start_state_monitor(
    instances: Arc<Mutex<HashMap<String, Instance>>>,
    adapter_states: Arc<Mutex<HashMap<String, crate::adapter::AdapterState>>>,
    registry: crate::adapter::AdapterRegistry,
    app_handle: tauri::AppHandle,
) {
    let mut notified_map: HashMap<String, bool> = HashMap::new();
    let mut transcript_mtimes: HashMap<String, u64> = HashMap::new();

    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(1));

            let mut map = instances.lock();
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let discovered = registry.discover_all();
            let mut new_map = HashMap::new();

            for raw in discovered {
                let id = raw.id.clone();
                let mut instance = Instance {
                    id: raw.id,
                    pid: raw.pid,
                    status: "waiting".to_string(),
                    working_directory: raw.working_directory,
                    terminal_app: raw.terminal_app,
                    last_activity: 0,
                    pending_permission: None,
                    last_hook_event: None,
                    subagents: Vec::new(),
                    model: None,
                    permission_mode: None,
                    context_percent: None,
                    conversation_log: Vec::new(),
                    session_start: None,
                    transcript_path: None,
                    session_name: raw.session_name.clone(),
                    wezterm_pane_id: None,
                    wt_session_id: None,
                    wezterm_unix_socket: None,
                    source: None,
                };
                if let Some(existing) = map.get(&id) {
                    instance.status = existing.status.clone();
                    instance.last_activity = existing.last_activity;
                    instance.pending_permission = existing.pending_permission.clone();
                    instance.last_hook_event = existing.last_hook_event.clone();
                    instance.subagents = existing.subagents.clone();
                    instance.model = existing.model.clone();
                    instance.permission_mode = existing.permission_mode.clone();
                    instance.context_percent = existing.context_percent;
                    instance.conversation_log = existing.conversation_log.clone();
                    instance.session_start = existing.session_start;
                    instance.transcript_path = existing.transcript_path.clone();
                    instance.session_name = existing.session_name.clone();
                    instance.wezterm_pane_id = existing.wezterm_pane_id.clone();
                    instance.wt_session_id = existing.wt_session_id.clone();
                    instance.wezterm_unix_socket = existing.wezterm_unix_socket.clone();
                    instance.source = existing.source.clone();
                }

                let adapter = registry.adapters.first().map(|a| a.as_ref());
                if let Some(adapter) = adapter {
                    let mut state = {
                        let states = adapter_states.lock();
                        states.get(&id).cloned().unwrap_or_default()
                    };

                    // Check transcript mtime for updates
                    if let Some(ref tp) = state.transcript_path {
                        let current_mtime = std::fs::metadata(tp)
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs());
                        if let Some(mtime) = current_mtime {
                            let changed = transcript_mtimes.get(tp).copied().unwrap_or(0) < mtime;
                            if changed {
                                transcript_mtimes.insert(tp.clone(), mtime);
                                let exchange = transcript::read_last_exchange(tp);
                                if let Some(user) = exchange.0 {
                                    state.latest_user_preview = Some(user);
                                }
                                if let Some(assistant) = exchange.1 {
                                    state.latest_assistant_preview = Some(assistant);
                                }
                                adapter_states.lock().insert(id.clone(), state.clone());
                            }
                        }
                    }

                    instance.conversation_log = adapter.get_preview(&state);
                    if state.wezterm_pane_id.is_some() {
                        instance.wezterm_pane_id = state.wezterm_pane_id.clone();
                    }
                    if state.wt_session_id.is_some() {
                        instance.wt_session_id = state.wt_session_id.clone();
                    }
                    if state.wezterm_unix_socket.is_some() {
                        instance.wezterm_unix_socket = state.wezterm_unix_socket.clone();
                    }
                    if state.source.is_some() {
                        instance.source = state.source.clone();
                    }
                    instance.subagents = adapter.get_subagents(&mut state);
                }

                new_map.insert(id.clone(), instance);
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
                            if inst.wezterm_pane_id.is_some() {
                                disc.wezterm_pane_id.clone_from(&inst.wezterm_pane_id);
                            }
                            if inst.wt_session_id.is_some() {
                                disc.wt_session_id.clone_from(&inst.wt_session_id);
                            }
                            if inst.wezterm_unix_socket.is_some() {
                                disc.wezterm_unix_socket.clone_from(&inst.wezterm_unix_socket);
                            }
                            if inst.source.is_some() {
                                disc.source.clone_from(&inst.source);
                            }
                            if inst.pending_permission.is_some() {
                                disc.pending_permission.clone_from(&inst.pending_permission);
                            }
                            if !inst.subagents.is_empty() {
                                for sa in &inst.subagents {
                                    if !disc.subagents.iter().any(|d| d.id == sa.id) {
                                        disc.subagents.push(sa.clone());
                                    }
                                }
                            }
                            if inst.status != "waiting" {
                                disc.status.clone_from(&inst.status);
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
                    let is_mid_tool = inst
                        .last_hook_event
                        .as_ref()
                        .map(|e| e.event == "PreToolUse")
                        .unwrap_or(false);

                    if inst.last_activity > 0
                        && now_secs - inst.last_activity > EXECUTING_TIMEOUT_SECS
                        && !is_mid_tool
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

#[cfg(target_os = "windows")]
fn start_hover_tracker(window: tauri::WebviewWindow, running: Arc<std::sync::atomic::AtomicBool>) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};
    thread::spawn(move || {
        let mut was_inside = false;
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(60));
            unsafe {
                let mut pt = windows::Win32::Foundation::POINT::default();
                let mut rect = RECT::default();
                let hwnd_val = match window.hwnd() {
                    Ok(h) => h.0 as isize,
                    Err(_) => continue,
                };
                if hwnd_val == 0 {
                    continue;
                }
                let hwnd = windows::Win32::Foundation::HWND(hwnd_val as *mut core::ffi::c_void);
                if GetCursorPos(&mut pt).is_ok() && GetWindowRect(hwnd, &mut rect).is_ok() {
                    let inside = pt.x >= rect.left && pt.x <= rect.right
                        && pt.y >= rect.top && pt.y <= rect.bottom;
                    if inside != was_inside {
                        was_inside = inside;
                        let _ = window.emit("pebble-hover", inside);
                    }
                }
            }
        }
    });
}

fn main() {
    let mut registry = AdapterRegistry::new();
    registry.register(std::sync::Arc::new(adapter::claude::ClaudeAdapter::new()));
    let _ = registry.configure_all();

    let instances = Arc::new(Mutex::new(HashMap::new()));
    let adapter_states: Arc<Mutex<HashMap<String, crate::adapter::AdapterState>>> = Arc::new(Mutex::new(HashMap::new()));
    let hover_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let permission_store = hook::server::PermissionResponseStore::new();

    let instances_for_hook = instances.clone();
    let adapter_states_for_hook = adapter_states.clone();
    let registry_for_hook = registry.clone();
    hook::server::start_hook_server(
        instances.clone(),
        permission_store.clone(),
        move |payload| {
            let hook_payload = adapter::HookPayload {
                event: payload.event.clone(),
                cwd: payload.cwd.clone(),
                timestamp: payload.timestamp,
                tool_name: payload.tool_name.clone(),
                tool_input: payload.tool_input.clone(),
                permission_mode: payload.permission_mode.clone(),
                tool_use_id: payload.tool_use_id.clone(),
                model: payload.raw_model.as_ref().and_then(|v| v.as_str().map(|s| s.to_string()))
                    .or_else(|| {
                        payload.context_window.as_ref().and_then(|cw| {
                            cw.as_object().and_then(|o| o.get("model")).and_then(|m| m.as_str().map(|s| s.to_string()))
                        })
                    }),
                context_percent: None,
                session_name: payload.session_name.clone(),
                transcript_path: payload.transcript_path.clone(),
                choices: payload.choices.clone(),
                default_choice: payload.default_choice.clone(),
                wezterm_pane_id: payload.wezterm_pane_id.clone(),
                wt_session_id: payload.wt_session_id.clone(),
                wezterm_unix_socket: payload.wezterm_unix_socket.clone(),
                agent_id: payload.agent_id.clone(),
                agent_type: payload.agent_type.clone(),
                source: payload.source.clone(),
                    };

            let adapter = match registry_for_hook.find_adapter_for_event(&hook_payload) {
                Some(a) => a,
                None => return,
            };

            let mut map = instances_for_hook.lock();
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let mut matched_id: Option<String> = None;
            if let Some(spid) = payload.sender_pid {
                let candidate_id = format!("cc-{spid}");
                if let Some(inst) = map.get(&candidate_id) {
                    if inst.working_directory == payload.cwd || is_related_cwd(&inst.working_directory, &payload.cwd) {
                        matched_id = Some(candidate_id);
                    }
                }
            }
            if matched_id.is_none() {
                if let Some(ref tp) = payload.transcript_path {
                    matched_id = map.values().find(|i| i.transcript_path.as_ref() == Some(tp)).map(|i| i.id.clone());
                }
            }
            if matched_id.is_none() {
                // Prefer blank instances (no transcript_path / no hook event) to avoid
                // cross-contaminating active instances when multiple sessions share cwd.
                let mut blank_candidate: Option<&Instance> = None;
                let mut max_la = 0u64;
                for i in map.values() {
                    if i.working_directory == payload.cwd {
                        if i.transcript_path.is_none() && i.last_hook_event.is_none() {
                            blank_candidate = Some(i);
                        } else if i.last_activity > max_la {
                            max_la = i.last_activity;
                            matched_id = Some(i.id.clone());
                        }
                    }
                }
                if matched_id.is_none() {
                    if let Some(i) = blank_candidate {
                        matched_id = Some(i.id.clone());
                    }
                }
            }
            if matched_id.is_none() {
                let mut blank_candidate: Option<&Instance> = None;
                let mut max_la = 0u64;
                for i in map.values() {
                    if is_related_cwd(&i.working_directory, &payload.cwd) {
                        if i.transcript_path.is_none() && i.last_hook_event.is_none() {
                            blank_candidate = Some(i);
                        } else if i.last_activity > max_la {
                            max_la = i.last_activity;
                            matched_id = Some(i.id.clone());
                        }
                    }
                }
                if matched_id.is_none() {
                    if let Some(i) = blank_candidate {
                        matched_id = Some(i.id.clone());
                    }
                }
            }

            if let Some(ref id) = matched_id {
                if let Some(mut instance) = map.remove(id) {
                    let mut states = adapter_states_for_hook.lock();
                    let mut adapter_state = states.remove(id).unwrap_or_default();
                    adapter.handle_hook(&hook_payload, &mut adapter_state, &mut map);
                    instance.status = adapter_state.status.clone();
                    instance.last_activity = adapter_state.last_activity.max(instance.last_activity);
                    instance.pending_permission = adapter_state.pending_permission.clone();
                    instance.last_hook_event = adapter_state.last_hook_event.clone();
                    instance.model = adapter_state.model.clone().or(instance.model.clone());
                    instance.permission_mode = adapter_state.permission_mode.clone().or(instance.permission_mode.clone());
                    instance.context_percent = adapter_state.context_percent.or(instance.context_percent);
                    instance.conversation_log = adapter_state.conversation_log.clone();
                    instance.session_start = adapter_state.session_start.or(instance.session_start);
                    instance.transcript_path = adapter_state.transcript_path.clone().or(instance.transcript_path.clone());
                    instance.session_name = adapter_state.session_name.clone().or(instance.session_name.clone());
                    instance.wezterm_pane_id = adapter_state.wezterm_pane_id.clone().or(instance.wezterm_pane_id.clone());
                    instance.wt_session_id = adapter_state.wt_session_id.clone().or(instance.wt_session_id.clone());
                    instance.wezterm_unix_socket = adapter_state.wezterm_unix_socket.clone().or(instance.wezterm_unix_socket.clone());
                    instance.source = adapter_state.source.clone().or(instance.source.clone());
                    instance.subagents = adapter.get_subagents(&mut adapter_state);
                    states.insert(id.clone(), adapter_state);
                    map.insert(id.clone(), instance);
                }
            } else {
                let (id, pid) = if let Some(spid) = payload.sender_pid {
                    (format!("cc-{}", spid), spid)
                } else {
                    (format!("cc-{}", payload.timestamp), 0)
                };
                let mut new_state = crate::adapter::AdapterState::default();
                adapter.handle_hook(&hook_payload, &mut new_state, &mut map);
                let instance = Instance {
                    id: id.clone(),
                    pid,
                    status: new_state.status.clone(),
                    working_directory: payload.cwd.clone(),
                    terminal_app: "Unknown".to_string(),
                    last_activity: now_secs,
                    pending_permission: new_state.pending_permission.clone(),
                    last_hook_event: new_state.last_hook_event.clone(),
                    subagents: adapter.get_subagents(&mut new_state),
                    model: new_state.model.clone(),
                    permission_mode: new_state.permission_mode.clone(),
                    context_percent: new_state.context_percent,
                    conversation_log: new_state.conversation_log.clone(),
                    session_start: new_state.session_start,
                    transcript_path: new_state.transcript_path.clone(),
                    session_name: new_state.session_name.clone(),
                    wezterm_pane_id: new_state.wezterm_pane_id.clone(),
                    wt_session_id: new_state.wt_session_id.clone(),
                    wezterm_unix_socket: new_state.wezterm_unix_socket.clone(),
                    source: new_state.source.clone(),
                };
                adapter_states_for_hook.lock().insert(id.clone(), new_state);
                map.insert(id, instance);
            }
        }
    );

    let prevent = tauri_plugin_prevent_default::Builder::new()
        .with_flags(tauri_plugin_prevent_default::Flags::CONTEXT_MENU)
        .build();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(prevent)
        .manage(AppState {
            instances: instances.clone(),
            registry: registry.clone(),
            adapter_states: adapter_states.clone(),
        })
        .manage(permission_store.clone())
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
                        let w = 324.0;
                        let x = (logical_width - w) / 2.0;
                        let _ = window.set_position(tauri::Position::Logical(
                            tauri::LogicalPosition { x, y: 0.0 }
                        ));
                        let _ = window.set_size(tauri::Size::Logical(
                            tauri::LogicalSize { width: w, height: 50.0 }
                        ));
                    }
                    let hr = hover_running.clone();
                    window.on_window_event(move |event| {
                        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                            hr.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    });
                    start_hover_tracker(window, hover_running.clone());
                }
            }
            // Setup system tray
            {
                let menu = tauri::menu::Menu::with_items(
                    app,
                    &[&tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?],
                )?;
                let tray_icon = tauri::image::Image::from_bytes(
                    include_bytes!("../icons/tray-icon.png")
                )?;
                let _tray = tauri::tray::TrayIconBuilder::new()
                    .icon(tray_icon)
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        if event.id.as_ref() == "quit" {
                            app.exit(0);
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            ..
                        } = event
                        {
                            if let Some(window) = tray.app_handle().get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                                let _ = window.emit("tray-toggle", ());
                            }
                        }
                    })
                    .build(app)?;
            }
            start_state_monitor(instances.clone(), adapter_states.clone(), registry.clone(), app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_instances, jump_to_terminal, get_instance_preview, resize_window_centered, bring_to_front])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
