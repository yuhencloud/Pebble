# Pebble 消息预览优化设计文档

## 日期
2026-04-14

## 背景与问题

当前 Pebble 卡片中的消息预览存在三个核心问题：
1. **噪声过多**：`transcript.rs` 的 `read_transcript_preview` 从 JSONL 文件末尾读取 64KB，过滤逻辑薄弱，会展示 thinking 块、工具结果、系统注入消息等无关内容。
2. **实时性不足**：Claude 的文本回复没有对应 hook，仅依赖 `UserPromptSubmit` / `PreToolUse` hook 驱动更新，导致用户发新消息后卡片内容不能及时同步。
3. **权限请求信息不足**：`needs_permission` 状态只显示简单的 "Allow Bash?"，缺少完整上下文，且没有自动吸引用户注意力的交互机制。

## 目标

- 卡片预览**清晰、精简、低噪声**
- 用户输入**毫秒级更新**，Claude 回复**1 秒内同步**
- 权限请求**信息完整、自动置顶、自动展开面板**

## 设计

### 1. 卡片预览规则

#### 1.1 正常状态（非权限请求）
固定最多两行：
- **第一行**：`You: {用户最近一次输入摘要}`（最多 60 字）
- **第二行**（按优先级）：
  1. 正在执行动作 → `Using {tool_name}` / `Agent {agent_type}`
  2. Claude 最新文本回复 → markdown 清洗后的开头摘要（最多 60 字）
  3. 无内容时隐藏第二行

#### 1.2 权限请求状态（`needs_permission`）
卡片垂直展开，展示完整权限信息：
- **第一行**：保留 `You: {用户最近一次输入}`
- **权限卡片区域**：
  - **结构化标题**：如 "Bash 命令执行请求"
  - **原始参数详情**：如完整的 Bash 命令、文件路径、工具入参 JSON（最多 300 字）
  - **选择按钮组**：映射 hook payload 的 `choices` 数组，等数量渲染；`default_choice` 高亮

### 2. 实时性机制

采用 **hook 即时 + 轮询兜底** 的混合策略：

| 场景 | 触发方式 | 延迟 |
|------|----------|------|
| 用户发消息 | `UserPromptSubmit` hook 直接携带 `tool_input` | 毫秒级 |
| 动作发生 | `PreToolUse` / `SubagentStart` / `PermissionRequest` hook | 毫秒级 |
| Claude 文本回复 | 后端 `start_state_monitor` 每秒检查 transcript `mtime` | ≤1 秒 |
| 子代理变化 | `SubagentStart` / `SubagentStop` hook + 文件 bootstrap | 毫秒级 |

后端检测到 transcript 更新时，重新解析最近一轮对话并 emit `instances-updated` 事件给前端。

### 3. 噪声过滤规则

重构 `transcript.rs` 解析逻辑，**仅保留**以下两类消息：
- **User 消息**：`type=user` 中的纯文本内容
  - 过滤 `<local-command-caveat>`、`<command-message>`
  - 过滤纯 `tool_result` 块（不展示工具结果）
- **Assistant 文本**：`type=assistant` 中的 `text` 块
  - 跳过 `tool_use`、`thinking` 块、空内容

#### 3.1 Markdown 清洗（Assistant 摘要）
提取 `text` 块后，去掉或替换以下符号，使预览更像自然语言：
- 代码块 `` ``` `` → 保留为 `Code: {语言}`
- 行内代码 `` ` `` → 去掉反引号
- 标题 `#` / `##` → 直接去掉
- 列表 `- ` / `* ` / `1. ` → 去掉前缀
- 加粗/斜体 `**` / `*` / `_` → 去掉标记符
- 链接 `[text](url)` → 只保留 `text`

### 4. 权限请求详情提取

`PendingPermission` 新增 `details: Option<String>` 字段。

`ClaudeAdapter::handle_hook` 在权限事件时，按 `tool_name` 提取详细上下文：

| Tool | 结构化标题示例 | details 内容 |
|------|----------------|--------------|
| Bash | Bash 命令执行请求 | 完整 `command` 字段 |
| Edit | 编辑文件请求 | `file_path` + `old_string` / `new_string` 摘要 |
| Write | 写入文件请求 | `file_path` + 内容前 200 字 |
| Read | 读取文件请求 | `file_path` |
| Delete | 删除文件请求 | `file_path` |
| 其他 | {tool_name} 请求 | `tool_input` JSON 摘要（前 200 字） |

前端 `permission-prompt` 拆分为：
1. `permission-title`：结构化标题
2. `permission-details`：原始参数详情（可滚动，最大高度 80px）
3. `permission-choices`：按钮组

### 5. 自动置顶与自动展开

#### 5.1 前端排序规则
`realInstances` 排序改为：
1. `status` 优先级：`needs_permission` > `executing` > `waiting` > `completed`
2. 再按 `working_directory` 字母顺序

使权限请求实例始终排在列表最前。

#### 5.2 自动展开面板
前端监听 `instances-updated` 事件。如果事件 payload 中包含任何 `status === "needs_permission"` 的实例，且当前面板处于收起状态，则自动调用 `expandPanel()` 展开。鼠标移出后仍按现有逻辑正常收起。

### 6. 后端改动文件

| 文件 | 改动内容 |
|------|----------|
| `pebble-app/src-tauri/src/transcript.rs` | 新增 `read_last_exchange(path)`；重构 `read_transcript_preview` 的过滤逻辑；新增 markdown 清洗函数 |
| `pebble-app/src-tauri/src/adapter/mod.rs` | `AdapterState` 新增 `latest_user_preview`、`latest_assistant_preview` |
| `pebble-app/src-tauri/src/adapter/claude.rs` | `handle_hook` 中从 `tool_input` 提取权限详情到 `PendingPermission.details`；`get_preview` 按新规则组装两行预览 |
| `pebble-app/src-tauri/src/types.rs` | `PendingPermission` 新增 `details: Option<String>` |
| `pebble-app/src-tauri/src/main.rs` | `start_state_monitor` 每秒检查 transcript `mtime`，有变化时重新解析并 emit 更新 |
| `pebble-app/src/App.tsx` | 排序规则更新；监听 `instances-updated` 自动展开；`permission-card` 拆分为 title + details |

## 验收标准

- [ ] 用户发送消息后，卡片第一行在 1 秒内更新为 `You: xxx`
- [ ] Claude 回复普通文本后，卡片第二行在 1 秒内更新为清洗后的摘要
- [ ] Claude 调用工具时，卡片第二行立即切换为 `Using {tool_name}`
- [ ] 不再展示 thinking 块、工具结果、`<command-message>` 等噪声
- [ ] 权限请求时，实例自动排到列表第一，面板自动展开
- [ ] 权限卡片展示结构化标题 + 原始参数详情 + 等数量的选择按钮
- [ ] `default_choice` 按钮高亮显示
