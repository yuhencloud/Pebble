import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

interface PendingPermission {
  tool_name: string;
  tool_use_id: string;
  prompt: string;
  choices: string[];
  default_choice?: string;
}

interface HookEvent {
  event: string;
  cwd: string;
  timestamp: number;
  tool_name?: string;
  tool_input?: unknown;
  permission_mode?: string;
  tool_use_id?: string;
}

interface Instance {
  id: string;
  pid: number;
  status: "waiting" | "executing" | "completed" | "needs_permission" | string;
  working_directory: string;
  terminal_app: string;
  last_activity: number;
  pending_permission?: PendingPermission;
  last_hook_event?: HookEvent;
}

const FILLET_R = 12;
const BODY_W = 300;
const COLLAPSED_W = BODY_W + FILLET_R * 2;
const COLLAPSED_H = 38 + FILLET_R;
const EXPANDED_W = 520 + FILLET_R * 2; // expanded width
const EXPANDED_H = 400 + FILLET_R;

function PixelFaceIcon({ status }: { status: string }) {
  const common = { width: 20, height: 20, viewBox: "0 0 20 20" };
  switch (status) {
    case "waiting":
      return (
        <svg {...common} className="pixel-face pixel-face--waiting">
          <rect x="2" y="4" width="16" height="12" rx="6" fill="currentColor" />
          <rect x="5" y="9" width="4" height="1" fill="#1a1a2e" />
          <rect x="11" y="9" width="4" height="1" fill="#1a1a2e" />
        </svg>
      );
    case "executing":
      return (
        <svg {...common} className="pixel-face pixel-face--executing">
          <rect x="2" y="4" width="16" height="12" rx="6" fill="currentColor" />
          <rect x="5" y="8" width="3" height="4" fill="#1a1a2e" />
          <rect x="12" y="8" width="3" height="4" fill="#1a1a2e" />
          <rect x="6" y="9" width="1" height="1" fill="#fff" />
          <rect x="13" y="9" width="1" height="1" fill="#fff" />
        </svg>
      );
    case "completed":
      return (
        <svg {...common} className="pixel-face pixel-face--completed">
          <rect x="2" y="4" width="16" height="12" rx="6" fill="currentColor" />
          <rect x="5" y="9" width="2" height="1" fill="#1a1a2e" />
          <rect x="7" y="8" width="1" height="1" fill="#1a1a2e" />
          <rect x="13" y="9" width="2" height="1" fill="#1a1a2e" />
          <rect x="12" y="8" width="1" height="1" fill="#1a1a2e" />
          <rect x="7" y="12" width="6" height="1" fill="#1a1a2e" />
        </svg>
      );
    case "needs_permission":
      return (
        <svg {...common} className="pixel-face pixel-face--needs_permission">
          <rect x="2" y="4" width="16" height="12" rx="6" fill="currentColor" />
          <rect x="5" y="8" width="3" height="4" fill="#1a1a2e" />
          <rect x="12" y="8" width="3" height="4" fill="#1a1a2e" />
          <rect x="6" y="9" width="1" height="1" fill="#fff" />
          <rect x="13" y="9" width="1" height="1" fill="#fff" />
          <rect x="8" y="13" width="4" height="1" fill="#1a1a2e" />
        </svg>
      );
    default:
      return (
        <svg {...common} className="pixel-face">
          <rect x="2" y="4" width="16" height="12" rx="6" fill="currentColor" />
        </svg>
      );
  }
}

function InstanceCard({
  inst,
  preview,
  onClick,
  onRespond,
}: {
  inst: Instance;
  preview?: string;
  onClick: () => void;
  onRespond?: (choice: string) => void;
}) {
  return (
    <div
      key={inst.id}
      className={`instance instance--${inst.status}`}
      onClick={onClick}
    >
      <div className="instance-row">
        <div className="instance-left">
          <PixelFaceIcon status={inst.status} />
        </div>
        <div className="instance-center">
          <div className="instance-preview" title={preview || ""}>
            {preview === undefined ? (
              <span className="preview-placeholder">...</span>
            ) : preview ? (
              preview
            ) : (
              <span className="preview-empty">No recent activity</span>
            )}
          </div>
        </div>
        <div className="instance-right">
          <span className="instance-badge">{inst.terminal_app}</span>
        </div>
      </div>
      {inst.status === "needs_permission" && inst.pending_permission && (
        <div className="permission-card" onClick={(e) => e.stopPropagation()}>
          <div className="permission-prompt">{inst.pending_permission.prompt}</div>
          <div className="permission-choices">
            {inst.pending_permission.choices.map((choice) => (
              <button
                key={choice}
                className={`permission-btn ${
                  inst.pending_permission?.default_choice === choice
                    ? "permission-btn--default"
                    : ""
                }`}
                onClick={() => onRespond?.(choice)}
              >
                {choice}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function App() {
  const [instances, setInstances] = useState<Instance[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [drawerVisible, setDrawerVisible] = useState(false);
  const [previews, setPreviews] = useState<Record<string, string>>({});
  const collapseTimer = useRef<number | null>(null);
  const expandTimer = useRef<number | null>(null);

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

  const fetchPreviews = async (ids: string[]) => {
    const results: Record<string, string> = {};
    await Promise.all(
      ids.map(async (id) => {
        try {
          const text = await invoke<string>("get_instance_preview", { instanceId: id });
          results[id] = text;
        } catch (e) {
          console.error("Failed to fetch preview:", e);
          results[id] = "";
        }
      })
    );
    setPreviews((prev) => ({ ...prev, ...results }));
  };

  const expandPanel = () => {
    if (collapseTimer.current) {
      window.clearTimeout(collapseTimer.current);
      collapseTimer.current = null;
    }
    if (!expanded) {
      setExpanded(true);
      invoke("resize_window_centered", { width: EXPANDED_W, height: EXPANDED_H, animate: false }).catch(console.error);
      expandTimer.current = window.setTimeout(() => {
        setDrawerVisible(true);
        const ids = realInstances.map((i) => i.id);
        if (ids.length) fetchPreviews(ids);
      }, 50);
    }
  };

  const collapsePanel = () => {
    if (expandTimer.current) {
      window.clearTimeout(expandTimer.current);
      expandTimer.current = null;
    }
    setDrawerVisible(false);
    if (collapseTimer.current) {
      window.clearTimeout(collapseTimer.current);
    }
    collapseTimer.current = window.setTimeout(() => {
      setExpanded(false);
      invoke("resize_window_centered", { width: COLLAPSED_W, height: COLLAPSED_H, animate: false }).catch(console.error);
      collapseTimer.current = null;
    }, 300);
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
  const W = expanded ? EXPANDED_W : COLLAPSED_W;
  const BR = W - R;
  const bodyH = expanded ? 400 : 38;
  const panelClip = `path('M 0,0 Q ${R},0 ${R},${R} L ${R},${bodyH - 12} Q ${R},${bodyH} ${R + 12},${bodyH} L ${BR - 12},${bodyH} Q ${BR},${bodyH} ${BR},${bodyH - 12} L ${BR},${R} Q ${BR},0 ${W},0 Z')`;

  return (
    <div
      className={`panel ${expanded ? "panel--open" : ""}`}
      onMouseEnter={expandPanel}
      onMouseLeave={collapsePanel}
      style={{ clipPath: panelClip }}
    >
      <div className="compact-header">
        <div className="wing wing--left">
          {
            (() => {
              let petMod = "";
              if (permissionCount > 0) petMod = "pet--alert";
              else if (executingCount > 0) petMod = "pet--executing";
              else if (realInstances.some((i) => i.status === "waiting")) petMod = "pet--waiting";
              else if (realInstances.some((i) => i.status === "completed")) petMod = "pet--completed";
              return (
                <div className={`pet ${petMod}`}>
                  <div className="pet-body">
                    <div className="pet-face">
                      <div className="pet-eye pet-eye--left" />
                      <div className="pet-eye pet-eye--right" />
                    </div>
                    <div className="pet-blush pet-blush--left" />
                    <div className="pet-blush pet-blush--right" />
                  </div>
                </div>
              );
            })()
          }
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
            <InstanceCard
              key={inst.id}
              inst={inst}
              preview={previews[inst.id]}
              onClick={() => jumpToTerminal(inst.id)}
              onRespond={(choice) => respondPermission(inst.id, choice)}
            />
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
