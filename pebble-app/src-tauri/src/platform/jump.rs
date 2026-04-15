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
        AllowSetForegroundWindow, ASFW_ANY, EnumWindows, GetWindowThreadProcessId,
        IsWindowVisible, SetForegroundWindow, SetWindowPos, ShowWindow,
        HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
    };

    unsafe fn get_foreground_window() -> windows::Win32::Foundation::HWND {
        if let Ok(user32) = GetModuleHandleA(windows::core::s!("user32.dll")) {
            if let Some(proc) = GetProcAddress(user32, windows::core::s!("GetForegroundWindow")) {
                type Fn = unsafe extern "system" fn() -> windows::Win32::Foundation::HWND;
                let f: Fn = std::mem::transmute(proc);
                return f();
            }
        }
        windows::Win32::Foundation::HWND(std::ptr::null_mut())
    }

    unsafe fn get_current_thread_id() -> u32 {
        if let Ok(kernel32) = GetModuleHandleA(windows::core::s!("kernel32.dll")) {
            if let Some(proc) = GetProcAddress(kernel32, windows::core::s!("GetCurrentThreadId")) {
                type Fn = unsafe extern "system" fn() -> u32;
                let f: Fn = std::mem::transmute(proc);
                return f();
            }
        }
        0
    }

    unsafe fn attach_thread_input(fg_thread: u32, current_thread: u32, attach: bool) {
        if let Ok(user32) = GetModuleHandleA(windows::core::s!("user32.dll")) {
            if let Some(proc) = GetProcAddress(user32, windows::core::s!("AttachThreadInput")) {
                type Fn = unsafe extern "system" fn(u32, u32, i32) -> i32;
                let f: Fn = std::mem::transmute(proc);
                let _ = f(fg_thread, current_thread, if attach { 1 } else { 0 });
            }
        }
    }

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
        let mut s = sysinfo::System::new();
        s.refresh_processes();
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
        // activate-pane automatically brings the pane (and its tab) to the foreground
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
        // Step 1: Terminal-specific precision jump (tab/pane switch within WezTerm)
        if terminal_app == "WezTerm" {
            if let Some(pane) = wezterm_pane_id {
                match activate_wezterm_pane(pane, wezterm_unix_socket) {
                    Ok(()) => {
                        eprintln!("[pebble-jump] WezTerm pane activated successfully");
                        return Ok(());
                    }
                    Err(e) => eprintln!("[pebble-jump] WezTerm pane activation failed: {}, falling back", e),
                }
            } else {
                eprintln!("[pebble-jump] WezTerm detected but no pane_id available");
            }
        }

        // Step 2: Window-level activation (bring terminal window to foreground)
        let mut hwnd = find_visible_window(pid);
        eprintln!("[pebble-jump] find_visible_window(pid={}) -> hwnd={:?}", pid, hwnd.0);
        if hwnd.0.is_null() {
            let terminal_pid = crate::platform::terminal::detect_terminal_pid(pid);
            eprintln!("[pebble-jump] detect_terminal_pid -> {}", terminal_pid);
            if terminal_pid != pid {
                hwnd = find_visible_window(terminal_pid);
                eprintln!("[pebble-jump] find_visible_window(terminal_pid={}) -> hwnd={:?}", terminal_pid, hwnd.0);
            }
        }
        if hwnd.0.is_null() {
            hwnd = find_window_walking_ancestors(pid);
            eprintln!("[pebble-jump] find_window_walking_ancestors(pid={}) -> hwnd={:?}", pid, hwnd.0);
        }

        if hwnd.0.is_null() {
            return Err("Window not found".to_string());
        }

        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            // Always restore the window first; some apps (e.g. WezTerm) may not
            // report iconic state correctly, and SetForegroundWindow alone will
            // not un-minimize a window.
            let restored = ShowWindow(hwnd, SW_RESTORE);
            eprintln!("[pebble-jump] ShowWindow SW_RESTORE -> {:?}", restored.0);
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
            // Temporarily set TOPMOST to bring window above Pebble's TOPMOST layer
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
            eprintln!("[pebble-jump] SetWindowPos TOPMOST done");

            // Attach thread input so SetForegroundWindow is allowed
            let fg_hwnd = get_foreground_window();
            let fg_thread = GetWindowThreadProcessId(fg_hwnd, None);
            let current_thread = get_current_thread_id();
            eprintln!("[pebble-jump] fg_hwnd={:?} fg_thread={} current_thread={}", fg_hwnd.0, fg_thread, current_thread);
            attach_thread_input(fg_thread, current_thread, true);
            let sf_result = SetForegroundWindow(hwnd);
            eprintln!("[pebble-jump] SetForegroundWindow -> {:?}", sf_result.0);
            attach_thread_input(fg_thread, current_thread, false);

            switch_to_this_window(hwnd, true);
            eprintln!("[pebble-jump] switch_to_this_window done");

            // Restore normal Z-order so the terminal doesn't stay permanently on top
            let _ = SetWindowPos(hwnd, windows::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST, 0, 0, 0, 0, flags);
            eprintln!("[pebble-jump] SetWindowPos NOTOPMOST done (restored normal z-order)");
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
