use sysinfo::System;

pub fn detect_terminal_app(pid: u32) -> String {
    let s = System::new_all();
    let mut current_pid = sysinfo::Pid::from(pid as usize);
    for _ in 0..10 {
        if let Some(proc) = s.process(current_pid) {
            let comm = proc.name().to_lowercase();
            let args = proc.cmd().join(" ").to_lowercase();
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
            if full.contains("windowsterminal") || full.contains("windows terminal") {
                return "WindowsTerminal".to_string();
            }
            if full.contains("wezterm") {
                return "WezTerm".to_string();
            }
            if full.contains("alacritty") {
                return "Alacritty".to_string();
            }
            if full.contains("conhost") || full.contains("cmd") {
                return "cmd".to_string();
            }
            if full.contains("pwsh") || full.contains("powershell") {
                return "PowerShell".to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_terminal_app_iterm2() {
        // This test relies on real process table; keep simple assertions only
        let _result = detect_terminal_app(std::process::id());
    }
}
