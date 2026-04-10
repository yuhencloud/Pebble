# Deep Interview Spec: Pebble — AI Coding Agent Monitor

## Metadata
- Interview ID: pebble-ai-monitor-001
- Rounds: 8
- Final Ambiguity Score: 13%
- Type: greenfield
- Generated: 2026-04-10
- Threshold: 20%
- Status: PASSED

## Clarity Breakdown
| Dimension | Score | Weight | Weighted |
|-----------|-------|--------|----------|
| Goal Clarity | 0.90 | 0.40 | 0.36 |
| Constraint Clarity | 0.85 | 0.30 | 0.255 |
| Success Criteria | 0.85 | 0.30 | 0.255 |
| **Total Clarity** | | | **0.87** |
| **Ambiguity** | | | **13%** |

## Goal
Build **Pebble**, an open-source, cross-platform desktop floating panel application that automatically discovers and monitors multiple Claude Code instances running across different terminals (iTerm2, tmux, Terminal.app, etc.), providing real-time status visibility, task completion notifications, and one-click terminal navigation — eliminating the pain of manually hunting through terminal windows/panes to find the right Claude Code session.

## Constraints
- **Tech Stack:** Tauri 2.0 (Rust backend) + React + TypeScript (frontend)
- **Platforms:** macOS, Windows, Linux (cross-platform via Tauri)
- **UI Form Factor:** Desktop floating panel / overlay widget (always-on-top, non-focus-stealing)
- **Data Source:** Claude Code hooks mechanism for event-driven state updates
- **License:** MIT
- **Open Source:** Yes, from day one
- **Performance Target:** Lightweight, minimal resource footprint (Tauri baseline ~10-30MB RAM)
- **First Agent Support:** Claude Code only (v1), extensible architecture for future agents (Codex, Cursor, Gemini CLI, etc.)

## Non-Goals (v1)
- GUI-based permission approval (v2)
- Markdown plan preview (v2)
- Sound alerts / 8-bit notifications (v2)
- Multi-theme support (v2)
- Support for AI agents other than Claude Code (v2+)
- Mobile or web-based interface
- MacBook notch-specific UI (Vibe Island style) — use standard floating panel instead

## Acceptance Criteria
- [ ] Automatically discover all running Claude Code instances on the system
- [ ] Display each instance with real-time status (waiting / executing / completed)
- [ ] Send system-level notification when a Claude Code instance completes its task
- [ ] Click on an instance in the panel to jump to / focus the corresponding terminal window/pane
- [ ] Floating panel stays on top without stealing focus from editor or terminal
- [ ] Works on macOS (primary), with architecture supporting Windows and Linux
- [ ] Application RAM usage under 50MB during normal operation
- [ ] Zero-config setup — detects Claude Code instances without user configuration

## Assumptions Exposed & Resolved
| Assumption | Challenge | Resolution |
|------------|-----------|------------|
| Need real-time monitoring | Claude Code already runs in terminal — why monitor? | Core value is multi-instance management: when running multiple CC sessions across terminals/tmux, manually locating the right one is painful |
| Need full Vibe Island feature parity | MVP could be much smaller | Simplified to 4 core features: discover, monitor, notify, navigate. Approval and plan preview deferred to v2 |
| Process monitoring for discovery | Could be fragile, depends on internals | Use Claude Code hooks mechanism instead — official, stable integration point |
| Need native UI for each platform | Would require platform-specific code | Tauri + React provides cross-platform with single codebase, floating panel via Tauri window APIs |

## Technical Context

### Architecture Overview
```
┌─────────────────────────────────────────┐
│           Pebble (Tauri App)            │
├──────────────────┬──────────────────────┤
│   Rust Backend   │   React Frontend     │
│                  │                      │
│  - Hook listener │  - Instance list UI  │
│  - Process disco │  - Status indicators │
│  - Terminal jump │  - Notification mgr  │
│  - IPC bridge    │  - Floating panel    │
└──────────────────┴──────────────────────┘
         │                    │
         ▼                    ▼
┌─────────────────┐  ┌───────────────────┐
│  Claude Code    │  │  System APIs      │
│  Hooks / Events │  │  - Notifications  │
│                 │  │  - Window mgmt    │
│                 │  │  - Terminal focus  │
└─────────────────┘  └───────────────────┘
```

### Key Technical Decisions
- **Tauri 2.0** for cross-platform desktop app with minimal resource usage
- **React + TypeScript** for frontend (largest ecosystem, most contributor-friendly)
- **Claude Code Hooks** as primary data source (event-driven, official API)
- **Rust backend** handles: hook event listening, process discovery, terminal window management, IPC
- **System notifications** via native OS APIs (through Tauri notification plugin)
- **Floating panel** via Tauri window config: `always_on_top`, `transparent`, `decorations: false`

### Claude Code Integration
- Use Claude Code hooks to receive events (tool use, task completion, permission requests)
- Hooks can be configured in `.claude/settings.json` to call external scripts/commands
- Pebble's Rust backend listens for these hook events (via local IPC: Unix socket or named pipe)
- Future: extend the hook/listener pattern for other AI agents

### Terminal Jump Implementation
- macOS: AppleScript / Accessibility API to focus iTerm2, Terminal.app windows
- Detect tmux sessions and panes associated with each Claude Code instance
- Future: Windows (COM automation), Linux (wmctrl/xdotool)

## Ontology (Key Entities)

| Entity | Type | Fields | Relationships |
|--------|------|--------|---------------|
| Claude Code Instance | Core domain | id, pid, status, working_directory, terminal_info, started_at | runs in Terminal Session, emits Hook Events |
| Monitor Panel | Core domain | position, size, visibility, instances_list | displays Claude Code Instances |
| Hook Event | Infrastructure | event_type, timestamp, instance_id, payload | emitted by Claude Code Instance, consumed by Pebble |
| Terminal Session | External system | terminal_app, window_id, pane_id, session_name | hosts Claude Code Instance |
| Notification | Supporting | title, body, instance_id, event_type, timestamp | triggered by Hook Event |
| Agent Status | Supporting | state (waiting/executing/completed), progress, last_activity | belongs to Claude Code Instance |
| Tauri App Shell | Infrastructure | window_config, system_tray, plugins | contains Monitor Panel |
| Permission Request | Supporting (v2) | request_type, description, instance_id | emitted by Claude Code Instance |

## Ontology Convergence

| Round | Entity Count | New | Changed | Stable | Stability Ratio |
|-------|-------------|-----|---------|--------|----------------|
| 1 | 3 | 3 | - | - | N/A |
| 2 | 4 | 1 | 0 | 3 | 75% |
| 3 | 5 | 1 | 0 | 4 | 80% |
| 4 | 7 | 2 | 1 | 4 | 71% |
| 5-7 | 8 | 1 | 0 | 7 | 87.5% |
| 8 | 8 | 0 | 0 | 8 | 100% |

## Interview Transcript
<details>
<summary>Full Q&A (8 rounds)</summary>

### Round 1
**Q:** What form factor for the cross-platform app?
**A:** Desktop floating panel / widget (like Vibe Island but cross-platform)
**Ambiguity:** 75.5% (Goal: 0.35, Constraints: 0.25, Criteria: 0.10)

### Round 2
**Q:** What does "done" look like for MVP?
**A:** Monitoring + Approval capabilities
**Ambiguity:** 60.5% (Goal: 0.50, Constraints: 0.25, Criteria: 0.40)

### Round 3
**Q:** Which tech stack for cross-platform?
**A:** Tauri (confirmed through discussion about performance, animation capabilities)
**Ambiguity:** 50% (Goal: 0.50, Constraints: 0.60, Criteria: 0.40)

### Round 4 (Contrarian Mode)
**Q:** Claude Code already runs in terminal — what's the real core value?
**A:** Multi-workflow management: multiple CC sessions across terminals/tmux are hard to locate manually. Need auto-discovery, completion notifications, and fast approval response.
**Ambiguity:** 37% (Goal: 0.75, Constraints: 0.60, Criteria: 0.50)

### Round 5
**Q:** Frontend framework preference? Open source license?
**A:** Recommended React (user is not familiar with frontend). MIT license.
**Ambiguity:** 31% (Goal: 0.75, Constraints: 0.80, Criteria: 0.50)

### Round 6 (Simplifier Mode)
**Q:** If you could only build one core feature first, which one?
**A:** Auto-discovery + status monitoring
**Ambiguity:** 24.5% (Goal: 0.80, Constraints: 0.80, Criteria: 0.65)

### Round 7
**Q:** How to get Claude Code status data?
**A:** Hooks mechanism — most stable, official integration point
**Ambiguity:** 21% (Goal: 0.85, Constraints: 0.80, Criteria: 0.70)

### Round 8
**Q:** Confirm specific MVP acceptance criteria?
**A:** Basic monitoring package: auto-discover, show status, notify on completion, click to jump to terminal
**Ambiguity:** 13% (Goal: 0.90, Constraints: 0.85, Criteria: 0.85)

</details>
