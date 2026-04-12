pub fn detect_terminal_app(pid: u32, ps_output: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_detect_terminal_app_iterm2() {
        let ps = "12345 1 iTerm2 /Applications/iTerm2.app\n";
        let result = detect_terminal_app(12345, ps);
        assert_eq!(result, "iTerm2");
    }
    #[test]
    fn test_detect_terminal_app_unknown() {
        let ps = "12345 1 foo /usr/bin/foo\n";
        let result = detect_terminal_app(12345, ps);
        assert_eq!(result, "Unknown");
    }
}
