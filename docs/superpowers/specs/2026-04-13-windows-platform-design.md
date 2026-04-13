# Pebble Windows 平台补全设计

**日期**: 2026-04-13  
**目标**: 在当前跨平台架构下，补全 Windows (`x86_64-pc-windows-msvc`) 的 platform 层实现，使 Pebble 能在 Windows 上编译、运行并完成核心功能（进程发现、CWD 获取、终端检测、窗口激活、hover 交互）。

---

## 1. 核心原则

1. **优先跨平台**：能用跨平台 crate（如 `sysinfo`）解决的，不用平台特化 API。
2. **平台隔离**：如果必须使用平台 API，代码必须放在 `#[cfg(target_os = "...")]` 分支内，禁止泄漏到通用路径。
3. **最小改动**：只做 Windows 补全，不动 macOS 现有行为。
4. **终端策略**：一级支持 **Windows Terminal**，其余终端通过 **PID 级窗口激活** 通用 fallback 兜底。

---

## 2. 编译阻塞项与修复

### 2.1 `icons/icon.ico`
Windows 打包需要 `icons/icon.ico`。已验证通过 ImageMagick/PIL 从 `icon.png` 生成，文件已存在于 `pebble-app/src-tauri/icons/icon.ico`。

### 2.2 `main.rs` 中 macOS-only 代码的 `cfg` 隔离
- `setup_notch_overlay` 是 macOS 刘海屏/菜单栏置顶特化逻辑，Windows 不需要等价实现（`tauri.conf.json` 已配置 `alwaysOnTop` + `decorations: false` + `transparent: true`，且 `main.rs` 里已有 `#[cfg(not(macos))]` 定位逻辑）。
- `setup_notch_overlay` 的**函数定义**目前未加 `#[cfg(target_os = "macos")]`，导致 Windows 编译报错。需要补上该属性。
- `main()` 中对 `setup_notch_overlay` 的调用在 `unsafe` 块内，该 `unsafe` 块本身被 `cfg(target_os = "macos")` 保护，理论上不应报错，但编译器似乎仍会扫描其中的 macOS 符号（`msg_send!`、`id`、`NSRect` 等）。需要将 `setup_notch_overlay` 整体声明为 macOS-only，彻底消除编译错误。

### 2.3 `pebble-bridge` Windows 二进制
- `pebble-bridge.rs` 是纯 Rust，依赖 `sysinfo` + `ureq`，在 Windows 下编译无问题。
- 流程：`cargo build --bin pebble-bridge` 后执行 `node scripts/build-bridge.cjs`，它会自动拷贝到 `bin/pebble-bridge-x86_64-pc-windows-msvc.exe`。

---

## 3. 平台模块改动

### 3.1 `platform/discovery.rs` — 进程发现

**问题**：当前硬编码 `ps -eo pid,ppid,comm,args`，Windows 没有 `ps`。

**方案**：用 `sysinfo` crate 重写 `list_processes()` 和 `find_claude_processes()`。

```rust
use sysinfo::{ProcessStatus, System};

pub fn list_processes() -> Vec<ProcessInfo> {
    let s = System::new_all();
    s.processes()
        .iter()
        .map(|(pid, proc)| ProcessInfo {
            pid: pid.as_u32(),
            ppid: proc.parent().map(|p| p.as_u32()).unwrap_or(0),
            comm: proc.name().to_string_lossy().into_owned(),
            args: proc.cmd().join(" "),
        })
        .collect()
}
```

- `sysinfo` 同时支持 macOS/Linux/Windows，重写后三个平台共用同一套代码。
- 删除旧的 `Command::new("ps")` 路径。

### 3.2 `platform/cwd.rs` — CWD 获取

**问题**：当前只有 macOS `lsof` fallback 和 Linux `/proc` fallback，Windows 返回 `None`。

**方案**：
1. session file 优先（保留，跨平台）。
2. fallback 改为 `sysinfo::Process::cwd()`，跨平台获取进程当前工作目录。
3. 删除旧的 `lsof` 和 `/proc` 硬编码分支（`sysinfo` 在底层已经做了平台适配）。

```rust
pub fn get_process_cwd(pid: u32) -> Option<String> {
    if pid == 0 { return None; }
    // 1. session file first
    // ... 保留现有逻辑 ...

    // 2. sysinfo fallback (cross-platform)
    let s = System::new_all();
    s.process(sysinfo::Pid::from(pid as usize))
        .and_then(|p| p.cwd().map(|p| p.to_string_lossy().into_owned()))
}
```

### 3.3 `platform/terminal.rs` — 终端应用检测

**问题**：只检测 iTerm2 / Terminal.app / tmux。

**方案**：PPID 链遍历逻辑保持不变，增加 Windows 特化的进程名匹配：

```rust
if full.contains("windowsterminal") || full.contains("windows terminal") {
    return "WindowsTerminal".to_string();
}
if full.contains("wezterm") { return "WezTerm".to_string(); }
if full.contains("alacritty") { return "Alacritty".to_string(); }
if full.contains("conhost") || full.contains("cmd") {
    return "cmd".to_string();
}
if full.contains("pwsh") || full.contains("powershell") {
    return "PowerShell".to_string();
}
```

### 3.4 `platform/jump.rs` — 终端窗口激活

**问题**：只有 macOS AppleScript 跳转。

**方案**：
- 保留现有的 macOS `#[cfg(target_os = "macos")]` 逻辑。
- 新增 Windows 分支：通过 `EnumWindows` 枚举顶层窗口，用 `GetWindowThreadProcessId` 匹配 PID，然后 `SetForegroundWindow` 激活。

```rust
#[cfg(target_os = "windows")]
pub fn jump_to_terminal(pid: u32, terminal_app: &str) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, LPARAM, BOOL};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, SetForegroundWindow,
        IsWindowVisible, GW_OWNER,
    };

    let target_pid = pid;
    let found = std::sync::Arc::new(std::sync::Mutex::new(None::<HWND>));
    let found_clone = found.clone();

    unsafe {
        EnumWindows(
            Some(std::mem::transmute(
                move |hwnd: HWND, _: LPARAM| -> BOOL {
                    if IsWindowVisible(hwnd).as_bool() {
                        let mut wpid = 0u32;
                        GetWindowThreadProcessId(hwnd, Some(&mut wpid));
                        if wpid == target_pid {
                            *found_clone.lock().unwrap() = Some(hwnd);
                            return false.into();
                        }
                    }
                    true.into()
                } as extern "system" fn(HWND, LPARAM) -> BOOL,
            )),
            LPARAM(0),
        ).ok();
    }

    if let Some(hwnd) = *found.lock().unwrap() {
        unsafe {
            SetForegroundWindow(hwnd);
        }
        Ok(())
    } else {
        Err("Window not found".to_string())
    }
}
```

> 注：实际实现中回调 closure 不能直接作为 extern 函数传给 `EnumWindows`，需用 static/trampoline 或单独定义 extern 函数。详细写法在 implementation plan 中精确定义。

- 如果匹配不到精确窗口，返回 `Err` 交由调用方静默处理（已有降级逻辑）。

### 3.5 `main.rs` — Windows hover tracker

**问题**：`start_hover_tracker` 只在 macOS 上实现，Windows 下 UI 收不到 `pebble-hover` 事件，无法展开面板。

**方案**：在 `main()` 的 `#[cfg(not(target_os = "macos"))]` 块中，增加 Windows hover tracker 线程。

Windows 实现：
- 使用 `windows` crate 的 `GetCursorPos` + `GetWindowRect`（或 `GetPhysicalCursorPos`）。
- 每秒检查多次（约 60ms 间隔）。
- 当鼠标进入/离开窗口区域时，emit `pebble-hover`。

```rust
#[cfg(target_os = "windows")]
fn start_hover_tracker(window: tauri::WebviewWindow, running: Arc<std::sync::atomic::AtomicBool>) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect};
    thread::spawn(move || {
        let mut was_inside = false;
        while running.load(std::sync::atomic::Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(60));
            unsafe {
                let mut pt = windows::Win32::Foundation::POINT::default();
                let mut rect = RECT::default();
                if GetCursorPos(&mut pt).is_ok() && GetWindowRect(window.hwnd().unwrap() as _, &mut rect).is_ok() {
                    let inside = pt.x >= rect.left && pt.x <= rect.right
                        && pt.y >= rect.top && pt.y <= rect.bottom;
                    if inside != was_inside {
                        was_inside = inside;
                        let _ = window.emit("pebble-hover", inside);
                    }
                }
            }
        }
    });
}
```

- `main()` 的非 macOS 设置块里，调用 `start_hover_tracker(window.clone(), hover_running.clone())`。
- macOS 的 `start_hover_tracker` 保持原样不动。

---

## 4. 依赖变更（Cargo.toml）

新增 Windows-only 依赖：

```toml
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.52", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
]}
```

其他依赖不变。`sysinfo` 已经存在于 `Cargo.toml` 中。

---

## 5. 验证步骤

1. `cargo check` → 0 errors, 0 warnings
2. `cargo test --lib` → 所有现有测试通过
3. `cargo build --bin pebble-bridge && node scripts/build-bridge.cjs` → Windows bridge 二进制正确生成
4. `npm run build && cargo tauri dev` → 应用能在 Windows 启动
5. 启动一个 Claude session（在 Windows Terminal 中），确认：
   - Pebble 列表中出现该 instance
   - CWD 正确
   - 终端类型显示为 `WindowsTerminal`
   - 鼠标悬停 Pebble 窗口时 UI 能展开
   - 点击 Pebble 窗口能跳回 Windows Terminal

---

## 6. 风险评估

| 风险 | 应对方案 |
|------|---------|
| `EnumWindows` 找到的是子窗口而非顶层窗口 | 结合 `IsWindowVisible` 和 `GetWindow` + `GW_OWNER` 过滤 |
| `sysinfo` 的 `cwd()` 在 Windows 上需要管理员权限 | 绝大多数普通用户进程（含 Claude）都可读取，无需提权 |
| `tauri::Window::hwnd()` 返回类型变化 | 使用 `as _` 做兼容转换 |
