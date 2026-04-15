# Tray Icon Design Spec

## Goal
Add a system tray icon for Pebble on both macOS and Windows. Left-clicking the icon toggles the Pebble floating panel between expanded and collapsed. Right-clicking opens a context menu with a single "Quit" item that exits the application.

## Architecture

### Components
- **Tray Icon**: Registered during Tauri `setup` via `tauri::tray::TrayIconBuilder`.
- **Tray Icon Asset**: A dedicated tray icon generated from the existing `icon.png`, removing the border while preserving the pixel-art capybara and original colors. The sprite fills the canvas to maximize visibility at small sizes. Target size: 256×256.
- **Tray Menu**: A minimal context menu created with `tauri::menu::Menu`, containing one item labeled "Quit".
- **Tray Event Handler**: Rust-side handler for left-click (show + focus window, emit toggle event) and right-click (show menu) events.

### Files to Touch
- `pebble-app/src-tauri/src/main.rs` — Core tray setup and event handling.
- `pebble-app/src-tauri/icons/icon.png` — Source asset for generating the tray icon.
- `pebble-app/src/App.tsx` — Listen for `tray-show-expand` event and trigger expand.

## Data Flow

### App Startup
```
setup() → TrayIconBuilder::new()
        → Load tray icon asset
        → Build menu (Quit)
        → Register event handler (left/right click)
```

### Left Click
1. Retrieve the "main" webview window.
2. Call `show()` and `set_focus()`.
3. Emit `tray-toggle` to the frontend.
4. Frontend receives the event and either expands or collapses the panel based on the current `expanded` state.
5. A 300ms debounce on the tray-toggle event prevents double-firing on Windows.
6. After expanding, hover-leave events are ignored for 800ms so the panel does not collapse immediately while the mouse is still in the tray area.

### Right Click
- Pop up the context menu showing the "Quit" item.

### Quit
- Call `app.exit(0)` to terminate Pebble completely.

## Error Handling
- **Missing icon asset**: Log an error with `eprintln!` and continue startup without a tray icon. Do not block the app from launching.
- **Window unavailable**: If the "main" window cannot be retrieved or recreated on left-click, silently skip and retry on the next click.
- **Platform differences**: Tauri v2's tray API abstracts macOS and Windows behavior; no platform-specific branches are required.

## Testing Checklist
- [ ] macOS menu bar shows the tray icon (pixel capybara, no border, colored).
- [ ] Windows system tray shows the tray icon.
- [ ] Left-clicking the tray icon toggles the Pebble panel between expanded and collapsed.
- [ ] Right-clicking the tray icon shows a context menu with "Quit".
- [ ] Clicking "Quit" fully exits the Pebble process.
- [ ] After closing the Pebble window, left-clicking the tray icon restores it and expands the panel.
