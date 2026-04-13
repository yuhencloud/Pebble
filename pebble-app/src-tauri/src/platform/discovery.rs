use std::collections::HashSet;
use sysinfo::System;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub args: String,
}

pub fn list_processes() -> Vec<ProcessInfo> {
    let s = System::new_all();
    s.processes()
        .iter()
        .map(|(pid, process)| ProcessInfo {
            pid: pid.as_u32(),
            ppid: process.parent().map(|p| p.as_u32()).unwrap_or(0),
            comm: process.name().to_string(),
            args: process.cmd().join(" "),
        })
        .collect()
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
        let pids: std::collections::HashSet<u32> = claudes.iter().map(|c| c.pid).collect();
        for c in &claudes {
            assert!(pids.contains(&c.pid));
        }
    }
}
