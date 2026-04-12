# Pebble 跨平台架构重构设计文档

**日期**: 2026-04-12  
**版本**: v0.1.0 → v0.2.0 重构  
**目标**: 将 Pebble 从 macOS/iTerm2/AppleScript 耦合架构，重构为跨平台（macOS/Windows/Linux）、支持多 CLI 工具、且与其他 Claude Code 插件互不干扰的架构。

---

## 1. 核心原则

1. **完全独立**：Pebble 的安装、运行、卸载完全不影响 claude-hud、ccswitch 等其他工具。`settings.json` 只写入 `hooks` 对象，绝不触碰 `statusLine` 或其他键。
2. **不读终端屏幕**：preview、subagent、permission 等信息不再通过 AppleScript 读取终端内容，改为从 transcript JSONL 和 session JSON 文件读取。
3. **跨平台权限响应**：通过 Claude Code 的 `PermissionRequest` hook 实现真正的双向响应，无需 AppleScript 按键注入或 `tmux send-keys`。
4. **Adapter 可扩展**：单体式 Rust backend 中引入 `Adapter` trait，当前只实现 `ClaudeAdapter`，未来可扩展 Codex、Gemini CLI 等。

---

## 2. 当前问题诊断

### 2.1 耦合点分析

| 问题 | 现状代码（v0.1.0） | 影响 |
|------|-------------------|------|
| `statusLine` 劫持 | `ensure_claude_hooks_config` 写入 `statusLine`，并通过 `pebble-bridge-statusline.sh` 转发数据给 claude-hud | 与 claude-hud 强耦合，覆盖其配置，违反独立性原则 |
| 终端屏幕抓取 | `get_instance_preview` 和 `parse_permission_choices` 通过 AppleScript 读取 iTerm2 session contents | 仅 macOS 可用，内容被 `clear` 后失效，不准确 |
| 权限响应注入 | `respond_permission` 仅支持 iTerm2，通过 AppleScript 注入上下箭头+回车 | Windows/Linux 完全不可用 |
| CWD 获取 | `get_process_cwd` 使用 `lsof` | macOS 特化命令，跨平台需替换 |
| Subagent 发现 | 通过 `ps` 进程树递归推断 | 无名称、无状态、不可靠 |

### 2.2 已验证的数据源

通过检查 `~/.claude/sessions/*.json` 和 `~/.claude/projects/<escaped-cwd>/<sessionId>.jsonl`，确认以下结构化数据完全可用且跨平台：

- **Session 文件**（`~/.claude/sessions/<pid>.json`）: `pid`, `sessionId`, `cwd`, `startedAt`, `name`
- **Transcript 文件**（`~/.claude/projects/<escaped-cwd>/<sessionId>.jsonl`）: 完整 `user` / `assistant` / `tool_use` / `tool_result` 消息
- **Subagent 文件**（`~/.claude/projects/<escaped-cwd>/subagents/`）: `<agentId>.jsonl` + `<agentId>.meta.json`

---

## 3. 架构设计

### 3.1 整体结构

```
pebble-app/src-tauri/src/
├── main.rs                    # Tauri setup、event emission、Adapter registry
├── adapter/
│   ├── mod.rs                 # Adapter trait + Registry
│   └── claude.rs              # ClaudeAdapter（唯一当前实现）
├── platform/
│   ├── mod.rs                 # 平台分发入口
│   ├── discovery.rs           # 进程发现（跨平台扩展）
│   ├── cwd.rs                 # CWD 解析（优先 session 文件）
│   ├── terminal.rs            # 终端应用检测
│   ├── jump.rs                # 终端窗口跳转（os-specific fallback）
│   └── notify.rs              # 系统通知（基本不变）
├── hook/
│   ├── server.rs              # HTTP hook server
│   └── bridge.rs              # pebble-bridge.mjs 生成与配置
├── transcript.rs              # JSONL transcript 读取与 preview 提取
└── session.rs                 # session 文件读取、subagent 解析
```

### 3.2 Adapter Trait

```rust
trait Adapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn auto_configure(&self) -> Result<(), String>;
    fn discover_instances(&self) -> Vec<RawInstance>;
    fn handle_hook(&self, payload: &HookPayload, state: &mut InstanceState);
    fn get_preview(&self, state: &InstanceState) -> Vec<String>;
    fn get_subagents(&self, state: &InstanceState) -> Vec<SubagentInfo>;
    fn jump_to_terminal(&self, instance: &Instance) -> Result<(), String>;
}
```

**说明**：
- `ClaudeAdapter` 将实现所有 Claude-specific 逻辑：hook 解析、transcript 读取、session 文件关联。
- 新增 CLI 工具时，只需新增一个 Adapter module 并注册到 Registry。

---

## 4. 通信方式

### 4.1 Hook → Pebble

保留 **HTTP localhost**（`127.0.0.1:9876`）：
- 跨平台兼容性好（Windows 对 Unix domain socket 支持不完善）
- `pebble-bridge.mjs`（Node.js）作为 bridge 脚本，读取 stdin JSON 后 POST 到 Pebble
- `handle_http_request` 需保留最新代码中的**循环读取**改进，防止大 payload truncation

### 4.2 PermissionRequest 双向响应

参考 `claude-island` 已验证的方案，向 `settings.json` 注册：

```json
"PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "node ~/.claude/hooks/pebble-bridge.mjs PermissionRequest", "timeout": 300 }] }]
```

当 Claude 触发 `PermissionRequest` 时：
1. bridge 脚本保持 HTTP 连接打开
2. Pebble hook server 读取事件，设置 instance 状态为 `needs_permission`
3. 用户在 UI 点击 Allow / Deny
4. Pebble 通过同一条 HTTP 连接返回 JSON：
   - `allow`: `{"hookSpecificOutput": {"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}`
   - `deny`: `{"hookSpecificOutput": {"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":"..."}}}`
5. bridge 脚本将 JSON 输出到 stdout，Claude Code 读取后执行批准/拒绝

**结果**：macOS/Windows/Linux 三端的权限响应完全一致，彻底删除 AppleScript 注入逻辑。

---

## 5. Hook 事件清单

Pebble 注册的事件（按优先级排序）：

| Hook | 用途 | 响应类型 |
|------|------|----------|
| `UserPromptSubmit` | 检测用户发送消息，状态变 `executing` | Fire-and-forget |
| `PreToolUse` | 工具调用开始，提取 `tool_name` / `tool_input` | Fire-and-forget |
| `PostToolUse` | 工具调用成功 | Fire-and-forget |
| `PostToolUseFailure` | 工具调用失败 | Fire-and-forget |
| **`PermissionRequest`** | **权限请求，UI 弹出卡片供用户一键确认** | **Blocking response** |
| `Stop` | 会话停止，状态回 `waiting` | Fire-and-forget |

**注意**：`StatusLine` **不注册、不使用**。`context_percent` 和 `model` 不再依赖此 hook。

---

## 6. 数据流重构

### 6.1 发现流程（Discovery）

**步骤**：
1. `ps -eo pid,ppid,comm,args` 扫描 `claude` / `claude-code` / `node claude-code` 进程
2. 对每个 PID，尝试读取 `~/.claude/sessions/<pid>.json`
3. 若存在 session 文件：直接获取 `sessionId`、`cwd`、`name`、`startedAt`
4. 若不存在：fallback 到平台特定 API（macOS `lsof`，Linux `/proc/<pid>/cwd`，Windows `NtQueryInformationProcess`）
5. Instance ID 格式保持 `cc-<pid>`

**优势**：绝大多数情况下无需平台 API 即可精准获取 CWD 和 Session ID。

### 6.2 Instance 匹配流程

Hook payload 到达后，按以下优先级匹配 instance：

1. `sender_pid` → `cc-<pid>` 精确匹配
2. `transcript_path` 与 instance.transcript_path 匹配
3. `cwd` 精确匹配（选 `last_activity` 最大的）
4. `cwd` 相关目录匹配（`is_related_cwd`）

### 6.3 Preview 获取

**彻底删除** `read_iterm2_last_lines`。

 Preview 来源：
1. 优先从 `transcript_path` 指向的 JSONL 文件读取末尾 N 条消息
2. 解析 `user` 和 `assistant` 类型消息（过滤 `thinking`、`command-message`、`<local-command-caveat>`）
3. `assistant` 的 `tool_use` 块展示为 `Using <tool_name>`
4. Fallback：使用 `last_hook_event.tool_input`

### 6.4 Subagent 获取

通过 `session.rs` 实现：
1. 根据 instance 的 `sessionId` + `cwd` 定位到 `~/.claude/projects/<escaped-cwd>/<sessionId>/subagents/`
2. 读取每个 `.meta.json` 获取 `agentType` 和 `description`
3. 读取对应的 `.jsonl` 推断状态（是否有新消息、是否完成）
4. 定期在 `start_state_monitor` 中刷新

### 6.5 Context% 与 Model

- **Context%**：初版**放弃显示**。不再注册 `StatusLine`，无法稳定获取实时 context 用量。后续若 Claude 在其他 hook 或文件中暴露此数据再恢复。
- **Model**：从 `PreToolUse` / `UserPromptSubmit` 的 hook payload 中随缘获取（部分版本会携带 `model` 字段）。若缺失，UI badge 显示为通用 "Claude"。

---

## 7. 平台层策略

### 7.1 终端跳转 (`jump_to_terminal`)

```rust
pub fn jump_to_terminal(instance: &Instance) -> Result<(), String> {
    match instance.terminal_app.as_str() {
        "iTerm2" => activate_iterm2_session(...),   // macOS AppleScript 保留
        "Terminal.app" => activate_terminal_app(...), // macOS AppleScript
        "tmux" => activate_tmux_pane(...),          // tmux select-pane
        "WindowsTerminal" => activate_windows_terminal(...),
        _ => activate_by_pid_fallback(instance.pid), // 尽量把窗口提到前台
    }
}
```

**降级原则**：如果精确跳转失败，至少尝试激活对应进程的前台窗口；若完全失败，静默处理不报错。

### 7.2 各平台实现对照

| 功能 | macOS | Linux | Windows |
|------|-------|-------|---------|
| 进程发现 | `ps` + session files | `ps` + `/proc/<pid>/cwd` + session files | `tasklist` / WMI + session files |
| CWD 获取 | `session_file.cwd` 优先 → `lsof` fallback | `session_file.cwd` 优先 → `/proc/<pid>/cwd` fallback | `session_file.cwd` 优先 → WMI fallback |
| 终端检测 | PPID 链遍历 | PPID 链 + `/proc/<pid>/comm` | 父进程遍历 |
| 权限响应 | Hook response ✅ | Hook response ✅ | Hook response ✅ |
| 终端跳转 | AppleScript / yabai(可选) | `wmctrl` / `xdotool` / 进程激活 | `SetForegroundWindow` / `wt.exe` |

---

## 8. UI 变化

### 8.1 移除的元素
- **Context% badge**：不再显示 `context_percent`（`<span className="badge badge--context">`）
- **iTerm2 特化提示**：由于权限响应改为跨平台 hook，UI 不需要在非 iTerm2 终端上降级提示

### 8.2 保留与增强
- **Instance 列表**: 继续展示 `working_directory`（或 `session_name` 优先），`model` badge，`terminal_app` badge，运行时长
- **Preview 行**: 更稳定，因为完全来自 transcript JSONL
- **Permission 卡片**: 继续红底展示，按钮触发 `respond_permission`，后端走 `PermissionRequest` hook response

---

## 9. 代码迁移计划

### 9.1 删除或废弃的函数/文件

| 目标 | 处理方式 |
|------|----------|
| `pebble-bridge-statusline.sh` | 删除 |
| `ensure_statusline_wrapper_script()` | 删除 |
| `read_iterm2_last_lines()` | 删除 |
| `parse_permission_choices()` | 删除 |
| `inject_permission_response_to_iterm2()` | 删除（权限走 hook response） |
| `get_process_cwd()` 的 `lsof` 主路径 | 降级为 fallback |
| `get_instance_preview()` | 删除，preview 逻辑并入 `ClaudeAdapter::get_preview()` |

### 9.2 Hook 配置更新

**新配置**（`ensure_claude_hooks_config` 写入）：

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "node ~/.claude/hooks/pebble-bridge.mjs UserPromptSubmit" }] }],
    "PreToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "node ~/.claude/hooks/pebble-bridge.mjs PreToolUse" }] }],
    "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "node ~/.claude/hooks/pebble-bridge.mjs PostToolUse" }] }],
    "PostToolUseFailure": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "node ~/.claude/hooks/pebble-bridge.mjs PostToolUseFailure" }] }],
    "PermissionRequest": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "node ~/.claude/hooks/pebble-bridge.mjs PermissionRequest", "timeout": 300 }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "node ~/.claude/hooks/pebble-bridge.mjs Stop" }] }]
  }
}
```

**关键行为**：
- 只修改 `settings.json` 的 `hooks` 键
- 使用 **append** 方式添加新事件（不覆盖其他工具已注册的同事件 hook）
- 若检测到 `statusLine` 被 Pebble 旧版占用，卸载时恢复原始状态

---

## 10. 测试策略

1. **macOS 回归测试**
   - iTerm2 跳转仍正常
   - 权限批准/拒绝 UI 正常工作（通过 `PermissionRequest` hook）
   - Session/Transcript 读取输出正确 preview

2. **跨平台编译测试**
   - `cargo check --target x86_64-pc-windows-msvc`
   - `cargo check --target x86_64-unknown-linux-gnu`
   - 确保 `#[cfg(target_os)]` 隔离代码无编译错误

3. **配置文件隔离测试**
   - 安装 Pebble 前已有 claude-hud 的 `settings.json`，验证 `statusLine` 不被覆盖
   - 安装 Pebble 后再运行 ccswitch 切换模型，验证 Pebble hooks 不被误删
   - 卸载 Pebble 后验证 `settings.json` 恢复干净

4. **Session 文件 fixture 测试**
   - 用固定的 `~/.claude/sessions/<pid>.json` 和 `projects/.../sessionId.jsonl` 测试 `SessionReader` 和 `TranscriptReader`

---

## 11. 后期扩展

### 11.1 多 CLI 支持
当需要支持 Codex / Gemini CLI / Cursor 等时：
1. 新建 `adapter/codex.rs` / `adapter/gemini.rs`
2. 实现各自的数据发现协议（可能是不同的 hook 机制、文件格式或 LSP）
3. 注册到 `AdapterRegistry`
4. UI 无需修改

### 11.2 Context% 恢复
若 Claude Code 未来：
- 在 `PreToolUse` 等 hook 中稳定携带 `context_percent`
- 或在 session/transcript 文件中写入 usage 数据
则可随时在 `ClaudeAdapter::handle_hook()` 中恢复 `context_percent` 的更新逻辑。

---

## 12. 总结

本次重构的核心不是增加功能，而是**解耦与换骨**：

- **从 `statusLine` 解放出来**：彻底不再依赖 `statusLine` hook，避免与 claude-hud 等工具冲突。
- **从 AppleScript 解放出来**：权限响应通过 `PermissionRequest` hook 实现真正的跨平台。
- **从终端屏幕抓取解放出来**：所有 preview 和 session 信息来自结构化文件（sessions + transcript）。
- **从单平台 CWD 探测解放出来**：通过 session 文件实现精准的跨平台进程-目录关联。

重构完成后，Pebble 将可以在 macOS、Windows、Linux 上运行，并为未来支持多种 AI CLI 工具奠定清晰的扩展边界。
