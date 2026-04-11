import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
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

const FILLET_R = 12;
const BODY_W = 300;
const COLLAPSED_W = BODY_W + FILLET_R * 2;
const COLLAPSED_H = 38 + FILLET_R;
const EXPANDED_W = COLLAPSED_W;
const EXPANDED_H = 400 + FILLET_R;

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

    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await listen<Instance[]>("instances-updated", (e) => {
        setInstances(e.payload);
      });
    })();

    return () => {
      clearInterval(interval);
      if (unlisten) unlisten();
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await listen<boolean>("pebble-hover", (e) => {
        if (e.payload) {
          expandPanel();
        } else {
          collapsePanel();
        }
      });
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const expandPanel = () => {
    if (collapseTimer.current) {
      window.clearTimeout(collapseTimer.current);
      collapseTimer.current = null;
    }
    if (!expanded) {
      setExpanded(true);
      appWindow.current.setSize(new LogicalSize(EXPANDED_W, EXPANDED_H));
    }
    window.setTimeout(() => setDrawerVisible(true), 50);
  };

  const collapsePanel = () => {
    setDrawerVisible(false);
    collapseTimer.current = window.setTimeout(() => {
      setExpanded(false);
      appWindow.current.setSize(new LogicalSize(COLLAPSED_W, COLLAPSED_H));
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

  const realInstances = useMemo(() => {
    return instances
      .filter((i) => i.pid !== 0)
      .sort((a, b) => a.working_directory.localeCompare(b.working_directory));
  }, [instances]);
  const executingCount = realInstances.filter((i) => i.status === "executing").length;
  const permissionCount = realInstances.filter((i) => i.status === "needs_permission").length;

  const R = FILLET_R;
  const W = COLLAPSED_W;
  const BR = W - R;
  const bodyH = expanded ? 400 : 38;
  const panelClip = `path('M 0,0 Q ${R},0 ${R},${R} L ${R},${bodyH - 12} Q ${R},${bodyH} ${R + 12},${bodyH} L ${BR - 12},${bodyH} Q ${BR},${bodyH} ${BR},${bodyH - 12} L ${BR},${R} Q ${BR},0 ${W},0 Z')`;

  return (
    <div
      className={`panel ${expanded ? "panel--expanded" : ""}`}
      onMouseEnter={expandPanel}
      onMouseLeave={collapsePanel}
      style={{ clipPath: panelClip }}
    >
      <div className="compact-header">
        <div className="wing wing--left">
          <div className={`pet ${permissionCount > 0 ? "pet--alert" : ""}`}>
            {permissionCount > 0 ? (
              <div className="pet-body">
                <span className="pet-mark">?</span>
              </div>
            ) : (
              <div className="pet-body">
                <div className="pet-eye pet-eye--left" />
                <div className="pet-eye pet-eye--right" />
              </div>
            )}
          </div>
        </div>
        <div className="wing wing--center">
          <span className="brand-text">Pebble</span>
        </div>
        <div className="wing wing--right">
          <div className="stats">
            {permissionCount > 0 && (
              <span className="stat stat--urgent">
                <span className="stat-dot stat-dot--urgent" />
                {permissionCount}
              </span>
            )}
            {executingCount > 0 && (
              <span className="stat stat--active">
                <span className="stat-dot stat-dot--active" />
                {executingCount}
              </span>
            )}
            {realInstances.length > 0 && (
              <span className="stat stat--idle">
                <span className="stat-dot stat-dot--idle" />
                {realInstances.length}
              </span>
            )}
          </div>
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
