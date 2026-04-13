use std::process::Command;

pub fn get_process_tty(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tty.is_empty() || tty == "??" {
        None
    } else {
        Some(tty)
    }
}

#[cfg(target_os = "macos")]
pub fn activate_iterm2() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        tell application "iTerm2"
            activate
        end tell
    "#;
    Command::new("osascript").arg("-e").arg(script).output()?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn activate_iterm2_session(tty: &str) -> Result<(), Box<dyn std::error::Error>> {
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
    Command::new("osascript").arg("-e").arg(&script).output()?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn activate_terminal_app() -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"
        tell application "Terminal"
            activate
        end tell
    "#;
    Command::new("osascript").arg("-e").arg(script).output()?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn activate_iterm2() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn activate_iterm2_session(_tty: &str) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn activate_terminal_app() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, ASFW_ANY, EnumWindows,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow,
        SetWindowPos, ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE,
        SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    };

    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    static mut FOUND_HWND: *mut c_void = std::ptr::null_mut();

    fn find_visible_window(pid: u32) -> HWND {
        TARGET_PID.store(pid, Ordering::SeqCst);
        unsafe {
            FOUND_HWND = std::ptr::null_mut();
            let _ = EnumWindows(Some(enum_proc_visible), LPARAM(0));
        }
        HWND(unsafe { FOUND_HWND })
    }

    fn is_valid_target(hwnd: HWND) -> bool {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return false;
            }
            let mut rect = windows::Win32::Foundation::RECT::default();
            if windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_err() {
                return false;
            }
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            width > 0 && height > 0
        }
    }

    extern "system" fn enum_proc_visible(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        unsafe {
            if !is_valid_target(hwnd) {
                return true.into();
            }
            let mut wpid = 0u32;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut wpid));
            if wpid == TARGET_PID.load(Ordering::SeqCst) {
                FOUND_HWND = hwnd.0;
                return false.into();
            }
            true.into()
        }
    }

    unsafe fn switch_to_this_window(hwnd: HWND, flash: bool) {
        if let Ok(user32) = GetModuleHandleA(windows::core::s!("user32.dll")) {
            if let Some(proc) = GetProcAddress(user32, windows::core::s!("SwitchToThisWindow")) {
                type SwitchFn = unsafe extern "system" fn(HWND, BOOL);
                let switch_fn: SwitchFn = std::mem::transmute(proc);
                switch_fn(hwnd, if flash { TRUE } else { false.into() });
            }
        }
    }

    /// Walk up the process tree from `start_pid`, trying to find a visible window
    /// at each ancestor. Returns the first HWND found, or a null HWND.
    fn find_window_walking_ancestors(start_pid: u32) -> HWND {
        let s = sysinfo::System::new_all();
        let mut current_pid = start_pid;
        for _ in 0..15 {
            let hwnd = find_visible_window(current_pid);
            if !hwnd.0.is_null() {
                return hwnd;
            }
            if let Some(proc) = s.process(sysinfo::Pid::from(current_pid as usize)) {
                if let Some(parent) = proc.parent() {
                    let ppid = parent.as_u32();
                    if ppid == 0 || ppid == current_pid {
                        break;
                    }
                    current_pid = ppid;
                    continue;
                }
            }
            break;
        }
        HWND(std::ptr::null_mut())
    }

    fn run_wezterm_cli(args: &[&str], unix_socket: Option<&str>) -> Result<(), String> {
        let mut cmd = std::process::Command::new("wezterm");
        cmd.args(args);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        if let Some(sock) = unix_socket {
            cmd.env("WEZTERM_UNIX_SOCKET", sock);
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let args_str = args.join(" ");
        std::thread::spawn(move || {
            let result = cmd.output();
            let _ = tx.send(result);
        });

        match rx.recv_timeout(std::time::Duration::from_millis(800)) {
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("wezterm cli {} failed: {}", args_str, stderr));
                }
                Ok(())
            }
            Ok(Err(e)) => Err(format!("Failed to run wezterm cli {}: {}", args_str, e)),
            Err(_) => Err(format!("wezterm cli {} timed out", args_str)),
        }
    }

    fn activate_wezterm_pane(pane_id: &str, unix_socket: Option<&str>) -> Result<(), String> {
        // First switch to the correct tab containing this pane
        if let Err(e) = run_wezterm_cli(&["cli", "activate-tab", "--pane-id", pane_id], unix_socket) {
            eprintln!("[pebble-jump] activate-tab failed: {}", e);
        }
        // Then focus the specific pane within that tab
        run_wezterm_cli(&["cli", "activate-pane", "--pane-id", pane_id], unix_socket)
    }

    pub fn jump_to_terminal(
        pid: u32,
        terminal_app: &str,
        wezterm_pane_id: Option<&str>,
        _wt_session_id: Option<&str>,
        wezterm_unix_socket: Option<&str>,
    ) -> Result<(), String> {
        eprintln!("[pebble-jump] pid={} terminal_app={} pane={:?} socket={:?}", pid, terminal_app, wezterm_pane_id, wezterm_unix_socket);
        // Terminal-specific precision jump (tab switch within WezTerm)
        if terminal_app == "WezTerm" {
            if let Some(pane) = wezterm_pane_id {
                match activate_wezterm_pane(pane, wezterm_unix_socket) {
                    Ok(()) => eprintln!("[pebble-jump] WezTerm pane activated, proceeding to window activation"),
                    Err(e) => eprintln!("[pebble-jump] WezTerm pane activation failed: {}", e),
                }
            } else {
                eprintln!("[pebble-jump] WezTerm detected but no pane_id available, falling back to window");
            }
        }
        // For WindowsTerminal, wt_session_id could be used here once
        // Microsoft adds a CLI flag to focus by session. For now, fall through.

        // Fallback: window-level activation
        let mut hwnd = find_visible_window(pid);
        if hwnd.0.is_null() {
            let terminal_pid = crate::platform::terminal::detect_terminal_pid(pid);
            if terminal_pid != pid {
                hwnd = find_visible_window(terminal_pid);
            }
        }
        if hwnd.0.is_null() {
            hwnd = find_window_walking_ancestors(pid);
        }

        if hwnd.0.is_null() {
            return Err("Window not found".to_string());
        }

        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
            switch_to_this_window(hwnd, true);
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use win::jump_to_terminal;

#[cfg(target_os = "macos")]
pub fn jump_to_terminal(
    pid: u32,
    terminal_app: &str,
    _wezterm_pane_id: Option<&str>,
    _wt_session_id: Option<&str>,
    _wezterm_unix_socket: Option<&str>,
) -> Result<(), String> {
    // Re-detect terminal if it was not identified during discovery
    let effective_app = if terminal_app == "Unknown" {
        let detected = crate::platform::terminal::detect_terminal_app(pid);
        eprintln!("[pebble-jump] terminal_app was Unknown, re-detected as: {}", detected);
        detected
    } else {
        terminal_app.to_string()
    };
    eprintln!("[pebble-jump] mac pid={} terminal_app={} effective={}", pid, terminal_app, effective_app);

    match effective_app.as_str() {
        "iTerm2" => {
            if let Some(tty) = get_process_tty(pid) {
                eprintln!("[pebble-jump] iTerm2 tty={}", tty);
                if let Err(e) = activate_iterm2_session(&tty) {
                    eprintln!("[pebble-jump] activate_iterm2_session failed: {}, falling back", e);
                    activate_iterm2().map_err(|e| e.to_string())?;
                }
            } else {
                eprintln!("[pebble-jump] no tty found for pid={}, activating iTerm2 generically", pid);
                activate_iterm2().map_err(|e| e.to_string())?;
            }
        }
        "Terminal.app" => {
            activate_terminal_app().map_err(|e| e.to_string())?;
        }
        other => {
            eprintln!("[pebble-jump] unhandled terminal_app: {}", other);
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn jump_to_terminal(
    _pid: u32,
    _terminal_app: &str,
    _wezterm_pane_id: Option<&str>,
    _wt_session_id: Option<&str>,
    _wezterm_unix_socket: Option<&str>,
) -> Result<(), String> {
    Ok(())
}
