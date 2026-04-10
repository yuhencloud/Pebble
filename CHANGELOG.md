# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- GUI-based permission approvals for Claude Code
  - `PreToolUse` hook integration to detect permission requests
  - Real-time iTerm2 terminal output scraping to parse exact menu choices
  - Pebble panel displays tool name, description, and all available choices
  - Default choice highlighted for quick one-click approval
  - One-click response injects the selected choice into the correct iTerm2 pane

## [0.1.0] - 2026-04-11

### Added
- Initial MVP release
- Tauri 2.0 + React + TypeScript application scaffold
- Auto-discovery of running Claude Code instances via process scanning
- Real-time status monitoring (`waiting` / `executing`) via Claude Code hooks
- macOS native system notifications on task completion
- Precise iTerm2 pane/tab jumping via AppleScript and TTY matching
- Always-on-top floating panel UI without focus stealing
- Automatic Claude Code hook configuration on first launch
- MIT License and open-source documentation
