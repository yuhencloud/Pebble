# Pebble

> 一款轻量、开源的 AI 编程助手浮动面板。基于 Tauri + React + TypeScript 构建。

<p align="center">
  <img src="pebble-app/src-tauri/icons/icon.png" width="120" alt="Pebble Logo">
</p>

Pebble 是一款跨平台桌面应用，用于监控多个在不同终端里运行的 Claude Code 会话。它解决了在 iTerm2 标签页、tmux 窗口或终端窗口之间来回翻找的痛点。

灵感来自 [Vibe Island](https://vibeisland.app)，但 **开源、轻量、跨平台**。

---

## 为什么用 Pebble？

如果你经常在多个 Claude Code 会话之间切换，你一定遇到过这些场景：

- 哪个标签页正在跑长任务？
- 哪个会话弹出了权限审批？
- 后台的任务什么时候跑完的？

Pebble 通过一个**非侵入式的浮动面板**解决这些问题：
- 自动发现系统里所有运行中的 Claude Code 实例
- 一眼看清所有会话的实时状态
- 任务完成后发送系统原生通知
- 一键跳转到对应的终端标签页
- 直接在面板上审批权限请求

---

## 功能特性

### 已实现
- **自动发现**：自动扫描系统中的 `claude` 进程
- **实时状态监控**：通过 Claude Code hooks 实时显示 `waiting` / `executing` / `needs_permission` 状态
- **系统通知**：任务完成后发送 macOS 原生通知
- **iTerm2 精准跳转**：点击实例即可通过 AppleScript 聚焦到对应的 iTerm2 标签页/窗格
- **权限审批**：直接在面板上响应 Claude Code 的权限请求（支持多选）
- **刘海风格面板**：顶部采用内凹圆角设计，与 MacBook 刘海自然融合
- **置顶浮动窗口**：始终可见，但不会抢夺编辑器焦点
- **零配置**：首次启动自动注册 Claude Code hooks

### 路线图
- [ ] 支持更多终端（Terminal.app、tmux 直连、Windows Terminal、Linux 终端）
- [ ] 支持更多 AI 助手（Codex、Cursor、Gemini CLI 等）
- [ ] Markdown 计划预览
- [ ] 声音提醒
- [ ] 拖拽调整窗口位置
- [ ] 签名 DMG 与 GitHub Releases

---

## 安装

### macOS

下载最新的 `Pebble.app` 从 [GitHub Releases](#)（即将发布），拖到 `应用程序` 文件夹即可。

或者从源码构建：

```bash
git clone https://github.com/yuhencloud/Pebble.git
cd Pebble/pebble-app
npm install
npm run tauri build -- --target aarch64-apple-darwin
```

构建完成后，应用位于：
```
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Pebble.app
```

### 环境要求
- macOS 14+（主要支持平台）
- Node.js 20+
- Rust 1.70+

### Windows / Linux

架构上基于 Tauri 是跨平台的，但 iTerm2 跳转和终端检测目前仅支持 macOS。欢迎贡献代码！

---

## 使用说明

1. **启动 Pebble**
2. 在 iTerm2（或其他终端）中**启动 Claude Code**
3. **观察面板** — Pebble 会自动列出你的会话
4. **点击实例** — 直接跳转到对应的 iTerm2 标签页
5. **向 Claude Code 发送消息** — 状态点会变为绿色（`executing`）
6. **等待约 8 秒** — 状态变回黄色（`waiting`），同时弹出系统通知
7. **权限审批** — 当面板上显示红色权限卡片时，直接点击选项即可审批

---

## 开发

```bash
cd pebble-app
npm install
npm run tauri dev
```

这会同时启动 Vite 开发服务器和 Tauri 应用（热重载模式）。

### 项目结构

```
pebble-app/
├── src/                   # React 前端
│   ├── App.tsx           # 主面板 UI
│   ├── App.css           # 面板样式
│   └── main.tsx          # React 入口
├── src-tauri/            # Rust 后端
│   ├── src/main.rs       # 核心逻辑（发现、hooks、iTerm2 跳转）
│   ├── Cargo.toml        # Rust 依赖
│   └── tauri.conf.json   # 应用窗口配置
├── package.json
├── vite.config.ts
└── ...
```

### 工作原理

```
┌─────────────────────────────────────────┐
│           Pebble (Tauri 应用)            │
├──────────────────┬──────────────────────┤
│    Rust 后端      │    React 前端        │
│                  │                      │
│  - Hook 监听器   │  - 实例列表 UI        │
│  - 进程发现      │  - 状态指示器         │
│  - 终端跳转      │  - 通知管理           │
│  - IPC 桥接     │  - 浮动面板            │
└──────────────────┴──────────────────────┘
         │                    │
         ▼                    ▼
┌─────────────────┐  ┌───────────────────┐
│   Claude Code   │  │     系统 API      │
│  Hooks / Events │  │  - 系统通知        │
│                 │  │  - 窗口管理        │
│                 │  │  - 终端聚焦        │
└─────────────────┘  └───────────────────┘
```

**进程发现**：Rust 后端通过 `ps` + `lsof` 查找 `claude` 进程及其工作目录。

**Hook 事件**：Pebble 启动本地 HTTP 服务器（`127.0.0.1:9876`），并在首次启动时自动往你的 `~/.claude/settings.json` 里写入 hook 命令。当 Claude Code 触发事件（`UserPromptSubmit`、`PostToolUse`、`Stop`）时，一个极小的 Node.js 桥接脚本会把事件转发给 Pebble。

**状态推断**：`UserPromptSubmit` 会将状态设为 `executing`。如果超过 30 秒没有新的事件到达，状态会恢复为 `waiting`，并触发系统通知。

**iTerm2 跳转**：点击某个实例时，Pebble 会读取该进程的 TTY，然后通过 AppleScript 遍历 iTerm2 的窗口/标签页/会话，精准聚焦到匹配的标签页。

---

## 参与贡献

欢迎提交 issue 或 pull request！

如果有较大的改动，建议先开一个 issue 讨论一下。

### 开发流程

1. Fork 本仓库
2. 创建特性分支（`git checkout -b feature/awesome-feature`）
3. 提交改动（`git commit -m 'Add awesome feature'`）
4. 推送到分支（`git push origin feature/awesome-feature`）
5. 发起 Pull Request

---

## 许可证

[MIT](LICENSE)

---

## 致谢

- 灵感来自 [Vibe Island](https://vibeisland.app) —— 一款优雅的 macOS 原生 AI 助手监控工具
- 基于 [Tauri](https://tauri.app/)、[React](https://react.dev/)、[Rust](https://www.rust-lang.org/) 构建
