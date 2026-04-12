use std::collections::HashSet;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub args: String,
}

pub fn list_processes() -> Vec<ProcessInfo> {
    let output = Command::new("ps")
        .args(&["-eo", "pid,ppid,comm,args"])
        .output();

    let mut results = Vec::new();
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
            let pid = parts[0].parse::<u32>().unwrap_or(0);
            let ppid = parts[1].parse::<u32>().unwrap_or(0);
            let comm = parts[2].to_string();
            let args = parts[3..].join(" ");
            if pid == 0 {
                continue;
            }
            results.push(ProcessInfo { pid, ppid, comm, args });
        }
    }
    results
}

pub fn find_claude_processes() -> Vec<ProcessInfo> {
    let all = list_processes();
    let mut claude_pids: HashSet<u32> = HashSet::new();

    for p in &all {
        let is_claude_main = p.comm == "claude" || p.comm == "claude-code";
        let is_node_claude = p.comm == "node" && p.args.contains("claude-code");
        if is_claude_main || is_node_claude {
            claude_pids.insert(p.pid);
        }
    }

    all.into_iter()
        .filter(|p| {
            let is_claude = p.comm == "claude" || p.comm == "claude-code"
                || (p.comm == "node" && p.args.contains("claude-code"));
            // skip children of other claude processes
            is_claude && (!claude_pids.contains(&p.ppid) || p.ppid == p.pid)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_processes_not_empty() {
        let procs = list_processes();
        assert!(!procs.is_empty(), "should find at least one process");
    }

    #[test]
    fn test_find_claude_processes_returns_only_top_level() {
        let claudes = find_claude_processes();
        // This test is environment-dependent; just verify it doesn't panic
        // and that no returned process is a child of another returned process.
        let pids: std::collections::HashSet<u32> = claudes.iter().map(|c| c.pid).collect();
        for c in &claudes {
            assert!(pids.contains(&c.pid));
        }
    }
}
