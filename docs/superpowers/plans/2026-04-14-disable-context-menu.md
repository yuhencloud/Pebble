# Disable Context Menu Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Disable the default browser right-click context menu across the entire Pebble application window.

**Architecture:** Add a `useEffect` hook in the root `App` component to listen for the `contextmenu` event on `document` and call `event.preventDefault()`. Clean up the listener on unmount.

**Tech Stack:** React, TypeScript, Tauri v2

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `pebble-app/src/App.tsx` | Modify | Add global `contextmenu` event listener to prevent default browser right-click menu |

---

### Task 1: Disable right-click context menu in App.tsx

**Files:**
- Modify: `pebble-app/src/App.tsx`

- [ ] **Step 1: Add contextmenu prevention useEffect**

  In `App.tsx`, inside the `App` component, add a new `useEffect` that registers a global `contextmenu` listener and cleans it up on unmount. Place it near the other top-level `useEffect` hooks.

  ```typescript
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      e.preventDefault();
    };
    document.addEventListener("contextmenu", handler);
    return () => {
      document.removeEventListener("contextmenu", handler);
    };
  }, []);
  ```

- [ ] **Step 2: Verify the change compiles**

  Run: `cd pebble-app && npx tsc --noEmit`
  Expected: No TypeScript errors.

- [ ] **Step 3: Commit**

  ```bash
  git add pebble-app/src/App.tsx
  git commit -m "fix(ui): disable default browser context menu"
  ```

---

## Self-Review Checklist

1. **Spec coverage:** The spec requires disabling the default browser right-click menu in the Tauri WebView. Task 1 directly implements this by adding a global `contextmenu` listener in `App.tsx`. ✓
2. **Placeholder scan:** No TBDs, TODOs, or vague instructions present. ✓
3. **Type consistency:** The event handler uses `MouseEvent`, which is correct for the `contextmenu` DOM event. ✓
