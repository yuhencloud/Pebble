# 禁用默认右键菜单设计文档

## 目标

禁用 Pebble 应用窗口中的浏览器默认右键菜单（如"检查元素"等）。

## 背景

Pebble 是基于 Tauri v2 + React 的桌面应用。Tauri WebView 默认会显示浏览器的右键上下文菜单，这在生产环境应用中是不必要的，且会影响用户体验。

经确认，Tauri v2 目前**没有** `tauri.conf.json` 配置项可以直接禁用右键菜单（相关功能请求 tauri-apps/tauri#8974 已被官方标记为 "not planned"）。因此必须通过 JavaScript 的 `preventDefault()` 来阻止。

## 方案

### 选定的方案

在前端 `pebble-app/src/App.tsx` 中通过事件监听器阻止默认右键菜单行为。

### 具体实现

- 在 `App` 组件的 `useEffect` 中，为 `document` 添加 `contextmenu` 事件监听器。
- 监听器内部调用 `event.preventDefault()`。
- 在清理函数中移除该监听器，避免内存泄漏。

### 影响范围

- 整个应用窗口内的所有区域都不会再弹出浏览器默认右键菜单。
- 只修改一个前端文件：`pebble-app/src/App.tsx`。

## 排除的方案

### Rust 层注入脚本

可以通过 `WebviewWindowBuilder::initialization_script()` 从 Rust 注入禁用脚本。但当前窗口由 `tauri.conf.json` 自动创建，改为手动构建需要重构较多代码，性价比不高。
