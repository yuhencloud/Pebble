use sysinfo::System;

fn is_terminal_process(comm: &str, args: &str) -> bool {
    let full = format!("{} {}", comm, args);
    full.contains("iterm2") || full.contains("iterm")
        || full.contains("terminal") || full.contains("apple_terminal")
        || full.contains("tmux")
        || full.contains("windowsterminal") || full.contains("windows terminal")
        || full.contains("wezterm")
        || full.contains("alacritty")
        || full.contains("conhost") || full.contains("cmd")
        || full.contains("pwsh") || full.contains("powershell")
}

pub fn detect_terminal_app(pid: u32) -> String {
    let s = System::new_all();
    let mut current_pid = sysinfo::Pid::from(pid as usize);
    for _ in 0..10 {
        if let Some(proc) = s.process(current_pid) {
            let comm = proc.name().to_lowercase();
            let args = proc.cmd().join(" ").to_lowercase();
            if is_terminal_process(&comm, &args) {
                if comm.contains("iterm2") || comm.contains("iterm") {
                    return "iTerm2".to_string();
                }
                if comm.contains("terminal") || comm.contains("apple_terminal") {
                    return "Terminal.app".to_string();
                }
                if comm.contains("tmux") {
                    return "tmux".to_string();
                }
                if comm.contains("windowsterminal") || comm.contains("windows terminal") {
                    return "WindowsTerminal".to_string();
                }
                if comm.contains("wezterm") {
                    return "WezTerm".to_string();
                }
                if comm.contains("alacritty") {
                    return "Alacritty".to_string();
                }
                if comm.contains("conhost") || comm.contains("cmd") {
                    return "cmd".to_string();
                }
                if comm.contains("pwsh") || comm.contains("powershell") {
                    return "PowerShell".to_string();
                }
            }
            if let Some(parent) = proc.parent() {
                if parent.as_u32() == 0 || parent.as_u32() == current_pid.as_u32() {
                    break;
                }
                current_pid = parent;
                continue;
            }
        }
        break;
    }
    "Unknown".to_string()
}

/// Returns the PID of the terminal process that owns the window.
/// On Windows, walks up to find the HIGHEST terminal process (e.g., WindowsTerminal.exe
/// rather than stopping at cmd.exe/powershell.exe which don't own windows).
pub fn detect_terminal_pid(pid: u32) -> u32 {
    let s = System::new_all();
    let mut current_pid = sysinfo::Pid::from(pid as usize);
    let mut best_terminal_pid = pid;
    for _ in 0..10 {
        if let Some(proc) = s.process(current_pid) {
            let comm = proc.name().to_lowercase();
            let args = proc.cmd().join(" ").to_lowercase();
            if is_terminal_process(&comm, &args) {
                best_terminal_pid = current_pid.as_u32();
                // On Windows, keep walking up to find window-owning parent
                // (e.g., WindowsTerminal.exe above powershell.exe)
                if cfg!(target_os = "windows") {
                    if let Some(parent) = proc.parent() {
                        if parent.as_u32() != 0 && parent.as_u32() != current_pid.as_u32() {
                            current_pid = parent;
                            continue;
                        }
                    }
                }
                return best_terminal_pid;
            }
            if let Some(parent) = proc.parent() {
                if parent.as_u32() == 0 || parent.as_u32() == current_pid.as_u32() {
                    break;
                }
                current_pid = parent;
                continue;
            }
        }
        break;
    }
    best_terminal_pid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_terminal_app_iterm2() {
        // This test relies on real process table; keep simple assertions only
        let _result = detect_terminal_app(std::process::id());
    }
}
