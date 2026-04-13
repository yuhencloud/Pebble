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

#[cfg(not(target_os = "macos"))]
pub fn activate_iterm2() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn activate_iterm2_session(_tty: &str) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    };

    static TARGET_PID: AtomicU32 = AtomicU32::new(0);
    static mut FOUND_HWND: *mut c_void = std::ptr::null_mut();

    pub fn jump_to_terminal(pid: u32, _terminal_app: &str) -> Result<(), String> {
        TARGET_PID.store(pid, Ordering::SeqCst);
        unsafe {
            FOUND_HWND = std::ptr::null_mut();
            let _ = EnumWindows(Some(enum_proc), LPARAM(0));
        }

        let hwnd = HWND(unsafe { FOUND_HWND });
        if hwnd.0.is_null() {
            return Err("Window not found".to_string());
        }
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(())
    }

    extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
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
}

#[cfg(target_os = "windows")]
pub use win::jump_to_terminal;

#[cfg(target_os = "macos")]
pub fn jump_to_terminal(pid: u32, terminal_app: &str) -> Result<(), String> {
    match terminal_app {
        "iTerm2" => {
            if let Some(tty) = get_process_tty(pid) {
                activate_iterm2_session(&tty).map_err(|e| e.to_string())?;
            } else {
                activate_iterm2().map_err(|e| e.to_string())?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn jump_to_terminal(_pid: u32, _terminal_app: &str) -> Result<(), String> {
    Ok(())
}
