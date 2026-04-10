import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import "./App.css";

interface PendingPermission {
  tool_name: string;
  tool_use_id: string;
  prompt: string;
  choices: string[];
  default_choice?: string;
}

interface Instance {
  id: string;
  pid: number;
  status: "waiting" | "executing" | "completed" | "needs_permission" | string;
  working_directory: string;
  terminal_app: string;
  last_activity: number;
  pending_permission?: PendingPermission;
}

const COLLAPSED_H = 52;
const EXPANDED_H = 400;

function App() {
  const [instances, setInstances] = useState<Instance[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [drawerVisible, setDrawerVisible] = useState(false);
  const collapseTimer = useRef<number | null>(null);
  const appWindow = useRef(getCurrentWindow());

  useEffect(() => {
    const load = async () => {
      try {
        const data = await invoke<Instance[]>("get_instances");
        setInstances(data);
      } catch (e) {
        console.error("Failed to load instances:", e);
      }
    };

    load();
    const interval = setInterval(load, 2000);
    return () => clearInterval(interval);
  }, []);

  const expandPanel = () => {
    if (collapseTimer.current) {
      window.clearTimeout(collapseTimer.current);
      collapseTimer.current = null;
    }
    if (!expanded) {
      setExpanded(true);
      appWindow.current.setSize(new LogicalSize(300, EXPANDED_H));
    }
    window.setTimeout(() => setDrawerVisible(true), 50);
  };

  const collapsePanel = () => {
    setDrawerVisible(false);
    collapseTimer.current = window.setTimeout(() => {
      setExpanded(false);
      appWindow.current.setSize(new LogicalSize(300, COLLAPSED_H));
    }, 250);
  };

  const jumpToTerminal = async (instanceId: string) => {
    try {
      await invoke("jump_to_terminal", { instanceId });
    } catch (e) {
      console.error("Failed to jump:", e);
    }
  };

  const respondPermission = async (instanceId: string, choice: string) => {
    try {
      await invoke("respond_permission", { instanceId, choice });
      setInstances((prev) =>
        prev.map((i) =>
          i.id === instanceId
            ? { ...i, status: "executing", pending_permission: undefined }
            : i
        )
      );
    } catch (e) {
      console.error("Failed to respond:", e);
    }
  };

  const realInstances = instances.filter((i) => i.pid !== 0);
  const executingCount = realInstances.filter((i) => i.status === "executing").length;
  const permissionCount = realInstances.filter((i) => i.status === "needs_permission").length;

  return (
    <div
      className="panel"
      onMouseEnter={expandPanel}
      onMouseLeave={collapsePanel}
    >
      <div className="compact-header">
        <div className="brand">
          <span className="brand-dot" />
          <span className="brand-text">Pebble</span>
        </div>
        <div className="summary">
          {permissionCount > 0 ? (
            <span className="summary-pill summary-pill--urgent">
              {permissionCount} pending
            </span>
          ) : executingCount > 0 ? (
            <span className="summary-pill summary-pill--active">
              {executingCount} active
            </span>
          ) : (
            <span className="summary-pill">
              {realInstances.length} idle
            </span>
          )}
        </div>
      </div>

      <div className={`drawer ${drawerVisible ? "drawer--visible" : ""}`}>
        <div className="instances">
          {realInstances.length === 0 && (
            <div className="empty">
              No Claude Code instances
              <div className="empty-sub">Start a session in iTerm2</div>
            </div>
          )}
          {realInstances.map((inst) => (
            <div
              key={inst.id}
              className={`instance instance--${inst.status}`}
              onClick={() => jumpToTerminal(inst.id)}
            >
              <div className="instance-header">
                <span className={`status-dot status-dot--${inst.status}`} />
                <span className="instance-status">
                  {inst.status === "needs_permission" ? "needs approval" : inst.status}
                </span>
                <span className="instance-pid">PID {inst.pid || "—"}</span>
              </div>
              <div className="instance-dir">{inst.working_directory}</div>
              {inst.status === "needs_permission" && inst.pending_permission && (
                <div
                  className="permission-card"
                  onClick={(e) => e.stopPropagation()}
                >
                  <div className="permission-prompt">
                    {inst.pending_permission.prompt}
                  </div>
                  <div className="permission-choices">
                    {inst.pending_permission.choices.map((choice) => (
                      <button
                        key={choice}
                        className={`permission-btn ${
                          inst.pending_permission?.default_choice === choice
                            ? "permission-btn--default"
                            : ""
                        }`}
                        onClick={() => respondPermission(inst.id, choice)}
                      >
                        {choice}
                      </button>
                    ))}
                  </div>
                </div>
              )}
              <div className="instance-term">{inst.terminal_app}</div>
            </div>
          ))}
        </div>

        <div className="footer">
          <div className="legend">
            <span className="legend-item">
              <span className="status-dot status-dot--waiting" /> wait
            </span>
            <span className="legend-item">
              <span className="status-dot status-dot--executing" /> exec
            </span>
            <span className="legend-item">
              <span className="status-dot status-dot--needs_permission" /> apr
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
