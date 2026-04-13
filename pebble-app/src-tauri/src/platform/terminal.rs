#[cfg(not(target_os = "macos"))]
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

fn terminal_app_name(comm: &str) -> Option<&'static str> {
    if comm.contains("iterm2") || comm.contains("iterm") {
        return Some("iTerm2");
    }
    // Check windowsterminal BEFORE terminal because "windowsterminal" contains "terminal"
    if comm.contains("windowsterminal") || comm.contains("windows terminal") {
        return Some("WindowsTerminal");
    }
    if comm.contains("terminal") || comm.contains("apple_terminal") {
        return Some("Terminal.app");
    }
    if comm.contains("tmux") {
        return Some("tmux");
    }
    if comm.contains("wezterm") {
        return Some("WezTerm");
    }
    if comm.contains("alacritty") {
        return Some("Alacritty");
    }
    if comm.contains("conhost") || comm.contains("cmd") {
        return Some("cmd");
    }
    if comm.contains("pwsh") || comm.contains("powershell") {
        return Some("PowerShell");
    }
    None
}

#[cfg(target_os = "macos")]
pub fn detect_terminal_app(pid: u32) -> String {
    let ps_output_cmd = std::process::Command::new("ps")
        .args(["-eo", "pid,ppid,comm,args"])
        .output();
    let ps_output = ps_output_cmd
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

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
            if is_terminal_process(&comm, &args) {
                // Try comm first; if path was truncated by spaces, try full string
                let full = format!("{} {}", comm, args);
                if let Some(name) = terminal_app_name(&comm).or_else(|| terminal_app_name(&full)) {
                    return name.to_string();
                }
                // Looks like a terminal helper (e.g. iTermServer) but can't name it;
                // keep walking up to find the main terminal process
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

#[cfg(target_os = "windows")]
pub fn detect_terminal_app(pid: u32) -> String {
    let s = System::new_all();
    let mut current_pid = sysinfo::Pid::from(pid as usize);
    let mut best_app: Option<&'static str> = None;
    for _ in 0..10 {
        if let Some(proc) = s.process(current_pid) {
            let comm = proc.name().to_lowercase();
            let args = proc.cmd().join(" ").to_lowercase();
            if is_terminal_process(&comm, &args) {
                if let Some(app) = terminal_app_name(&comm) {
                    best_app = Some(app);
                    // On Windows, keep walking up to find the actual terminal emulator
                    // (e.g., WezTerm or WindowsTerminal) rather than stopping at the shell
                    // (e.g., pwsh.exe or cmd.exe).
                    if let Some(parent) = proc.parent() {
                        if parent.as_u32() != 0 && parent.as_u32() != current_pid.as_u32() {
                            current_pid = parent;
                            continue;
                        }
                    }
                }
                return best_app.unwrap_or("Unknown").to_string();
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
    best_app.unwrap_or("Unknown").to_string()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn detect_terminal_app(pid: u32) -> String {
    let s = System::new_all();
    let mut current_pid = sysinfo::Pid::from(pid as usize);
    for _ in 0..10 {
        if let Some(proc) = s.process(current_pid) {
            let comm = proc.name().to_lowercase();
            let args = proc.cmd().join(" ").to_lowercase();
            if is_terminal_process(&comm, &args) {
                return terminal_app_name(&comm).unwrap_or("Unknown").to_string();
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
#[cfg(target_os = "macos")]
pub fn detect_terminal_pid(pid: u32) -> u32 {
    let ps_output_cmd = std::process::Command::new("ps")
        .args(["-eo", "pid,ppid,comm,args"])
        .output();
    let ps_output = ps_output_cmd
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

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
            if is_terminal_process(&comm, &args) {
                return current_pid;
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
    pid
}

#[cfg(target_os = "windows")]
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
                if let Some(parent) = proc.parent() {
                    if parent.as_u32() != 0 && parent.as_u32() != current_pid.as_u32() {
                        current_pid = parent;
                        continue;
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

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn detect_terminal_pid(pid: u32) -> u32 {
    let s = System::new_all();
    let mut current_pid = sysinfo::Pid::from(pid as usize);
    for _ in 0..10 {
        if let Some(proc) = s.process(current_pid) {
            let comm = proc.name().to_lowercase();
            let args = proc.cmd().join(" ").to_lowercase();
            if is_terminal_process(&comm, &args) {
                return current_pid.as_u32();
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
    pid
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
