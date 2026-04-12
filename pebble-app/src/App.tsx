import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

interface SubagentInfo {
  id: string;
  status: string;
  name: string;
}

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
  model?: string;
  context_percent?: number;
}

interface Instance {
  id: string;
  pid: number;
  status: "waiting" | "executing" | "completed" | "needs_permission";
  working_directory: string;
  terminal_app: string;
  last_activity: number;
  pending_permission?: PendingPermission;
  last_hook_event?: HookEvent;
  subagents: SubagentInfo[];
  model?: string;
  permission_mode?: string;
  context_percent?: number;
  conversation_log?: string[];
  session_start?: number;
}

const FILLET_R = 12;
const BODY_W = 300;
const COLLAPSED_W = BODY_W + FILLET_R * 2;
const COLLAPSED_H = 38 + FILLET_R;
const EXPANDED_W = 520 + FILLET_R * 2;
const MAX_EXPANDED_BODY_H = 420;

function formatTimeAgo(ts: number): string {
  if (ts <= 0) return "—";
  const diff = Math.floor(Date.now() / 1000) - ts;
  if (diff < 60) return "<1m";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

function formatDuration(startTs: number): string {
  if (startTs <= 0) return "—";
  const diff = Math.floor(Date.now() / 1000) - startTs;
  if (diff < 60) return "<1m";
  const m = Math.floor(diff / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const remM = m % 60;
  if (h < 24) return remM > 0 ? `${h}h ${remM}m` : `${h}h`;
  const d = Math.floor(h / 24);
  return `${d}d`;
}

function getCurrentAction(inst: Instance): string | null {
  if (!inst.last_hook_event) return null;
  const ev = inst.last_hook_event;
  if (ev.event === "PreToolUse" && ev.tool_name) {
    return `Agent ${ev.tool_name}`;
  }
  if (ev.event === "UserPromptSubmit") {
    return null; // handled in user message row
  }
  if (ev.event === "PostToolUse") {
    return `${ev.tool_name || "Tool"} completed`;
  }
  if (ev.event === "PostToolUseFailure") {
    return `${ev.tool_name || "Tool"} failed`;
  }
  return null;

}

function getUserMessage(inst: Instance): string | null {
  if (!inst.last_hook_event) return null;
  const ev = inst.last_hook_event;
  if (ev.event === "UserPromptSubmit") {
    if (ev.tool_input) {
      let text = String(ev.tool_input);
      if (text.length > 90) text = text.slice(0, 90) + "...";
      return `You: ${text}`;
    }
    return "You: ...";
  }
  return null;
}

function StatusDot({ status }: { status: string }) {
  return <span className={`status-dot status-dot--${status}`} />;
}

function MiniPet({ status }: { status: string }) {
  let mod = "";
  if (status === "needs_permission") mod = "mini-pet--alert";
  else if (["waiting", "executing", "completed"].includes(status)) mod = `mini-pet--${status}`;
  return (
    <div className={`mini-pet ${mod}`}>
      <div className="mini-pet-body">
        <div className="mini-pet-face">
          <div className="mini-pet-eye mini-pet-eye--left" />
          <div className="mini-pet-eye mini-pet-eye--right" />
        </div>
        <div className="mini-pet-blush mini-pet-blush--left" />
        <div className="mini-pet-blush mini-pet-blush--right" />
      </div>
    </div>
  );
}

function InstanceCard({
  inst,
  onClick,
  onRespond,
  onSubagentClick,
}: {
  inst: Instance;
  onClick: () => void;
  onRespond?: (choice: string) => void;
  onSubagentClick?: (id: string) => void;
}) {
  const [responding, setResponding] = useState(false);

  const displayPreview = useMemo(() => {
    if (inst.conversation_log && inst.conversation_log.length > 0) {
      return inst.conversation_log.join("\n");
    }
    const msg = getUserMessage(inst);
    if (msg) return msg;
    const action = getCurrentAction(inst);
    if (action) return action;
    return "No recent activity";
  }, [inst]);

  const dirName = inst.working_directory.split("/").pop() || inst.working_directory;
  const runtime = inst.session_start != null ? formatDuration(inst.session_start) : formatTimeAgo(inst.last_activity);

  const handleRespond = async (choice: string) => {
    if (responding || !onRespond) return;
    setResponding(true);
    try {
      await onRespond(choice);
    } finally {
      setResponding(false);
    }
  };

  return (
    <div className={`instance instance--${inst.status}`}>
      <div className="instance-main" onClick={onClick}>
        <MiniPet status={inst.status} />
        <div className="instance-content">
          <div className="instance-title-row">
            <span className="instance-dir" title={inst.working_directory}>
              {dirName}
            </span>
            <div className="instance-badges">
              <span className="badge badge--agent">{inst.model || "Claude"}</span>
              {inst.context_percent != null && (
                <span className="badge badge--context">{inst.context_percent}%</span>
              )}
              {inst.permission_mode && (
                <span className="badge badge--mode">{inst.permission_mode}</span>
              )}
              <span className="badge badge--terminal">{inst.terminal_app}</span>
              <span className="badge badge--time">{runtime}</span>
            </div>
          </div>

          {displayPreview && (
            <div className="instance-preview-row">
              <span className="instance-preview-text" title={displayPreview}>
                {displayPreview}
              </span>
            </div>
          )}
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
                onClick={() => handleRespond(choice)}
                disabled={responding}
              >
                {choice}
              </button>
            ))}
          </div>
        </div>
      )}

      {inst.subagents.length > 0 && (
        <div className="subagents">
          <div className="subagents-title">Subagents ({inst.subagents.length})</div>
          <div className="subagents-list">
            {inst.subagents.map((sub) => (
              <div
                key={sub.id}
                className={`subagent subagent--${sub.status}`}
                onClick={() => onSubagentClick?.(sub.id)}
              >
                <StatusDot status={sub.status} />
                <span className="subagent-name">{sub.name}</span>
                <span className="subagent-status">{sub.status === "completed" ? "Done" : sub.status}</span>
              </div>
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
  const collapseTimer = useRef<number | null>(null);
  const expandTimer = useRef<number | null>(null);
  const resizeDebounceTimer = useRef<number | null>(null);
  const wasExpandedRef = useRef(false);

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

  const expandPanelRef = useRef<() => void>(() => {});
  const collapsePanelRef = useRef<() => void>(() => {});

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await listen<boolean>("pebble-hover", (e) => {
        if (e.payload) {
          expandPanelRef.current();
        } else {
          collapsePanelRef.current();
        }
      });
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const realInstances = useMemo(() => {
    return instances
      .filter((i) => i.pid !== 0 || (i.last_activity > 0 && !!i.last_hook_event))
      .map((i) => ({ ...i, subagents: i.subagents || [] }))
      .sort((a, b) => a.working_directory.localeCompare(b.working_directory));
  }, [instances]);
  const executingCount = realInstances.filter((i) => i.status === "executing").length;
  const permissionCount = realInstances.filter((i) => i.status === "needs_permission").length;
  const petMod = useMemo(() => {
    if (permissionCount > 0) return "pet--alert";
    if (executingCount > 0) return "pet--executing";
    if (realInstances.some((i) => i.status === "waiting")) return "pet--waiting";
    if (realInstances.some((i) => i.status === "completed")) return "pet--completed";
    return "";
  }, [permissionCount, executingCount, realInstances]);

  const instancesRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);
  const [desiredBodyH, setDesiredBodyH] = useState(COLLAPSED_H - FILLET_R);

  useLayoutEffect(() => {
    if (!expanded || !drawerVisible) return;
    const isFirstExpand = expanded && !wasExpandedRef.current;
    wasExpandedRef.current = expanded;

    const doResize = () => {
      const headerH = 38;
      const paddingB = 12;
      const contentHeight = innerRef.current?.offsetHeight ?? 0;
      const contentH = contentHeight + headerH + paddingB;
      const h = Math.min(Math.max(contentH, COLLAPSED_H - FILLET_R), MAX_EXPANDED_BODY_H);
      setDesiredBodyH(h);
      invoke("resize_window_centered", { width: EXPANDED_W, height: h + FILLET_R, animate: false }).catch(console.error);
    };

    if (isFirstExpand) {
      if (resizeDebounceTimer.current) {
        window.clearTimeout(resizeDebounceTimer.current);
        resizeDebounceTimer.current = null;
      }
      requestAnimationFrame(() => doResize());
    } else {
      if (resizeDebounceTimer.current) {
        window.clearTimeout(resizeDebounceTimer.current);
      }
      resizeDebounceTimer.current = window.setTimeout(() => {
        doResize();
        resizeDebounceTimer.current = null;
      }, 50);
    }

    const ro = new ResizeObserver(() => {
      if (resizeDebounceTimer.current) {
        window.clearTimeout(resizeDebounceTimer.current);
      }
      resizeDebounceTimer.current = window.setTimeout(() => {
        doResize();
        resizeDebounceTimer.current = null;
      }, 50);
    });
    if (innerRef.current) {
      ro.observe(innerRef.current);
    }
    return () => {
      ro.disconnect();
      if (resizeDebounceTimer.current) {
        window.clearTimeout(resizeDebounceTimer.current);
        resizeDebounceTimer.current = null;
      }
    };
  }, [expanded, drawerVisible, realInstances]);

  const expandPanel = () => {
    if (collapseTimer.current) {
      window.clearTimeout(collapseTimer.current);
      collapseTimer.current = null;
    }
    if (!expanded) {
      setExpanded(true);
      setDrawerVisible(true);
    } else if (!drawerVisible) {
      setDrawerVisible(true);
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
      wasExpandedRef.current = false;
      invoke("resize_window_centered", { width: COLLAPSED_W, height: COLLAPSED_H, animate: false }).catch(console.error);
      collapseTimer.current = null;
    }, 150);
  };

  expandPanelRef.current = expandPanel;
  collapsePanelRef.current = collapsePanel;

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


  const R = FILLET_R;
  const W = expanded ? EXPANDED_W : COLLAPSED_W;
  const BR = W - R;
  const bodyH = expanded ? desiredBodyH : 38;
  // Safer clip path that handles small heights
  const safeBottomY = Math.max(bodyH, R + 12);
  const panelClip = `path('M 0,0 Q ${R},0 ${R},${R} L ${R},${safeBottomY - 12} Q ${R},${safeBottomY} ${R + 12},${safeBottomY} L ${BR - 12},${safeBottomY} Q ${BR},${safeBottomY} ${BR},${safeBottomY - 12} L ${BR},${R} Q ${BR},0 ${W},0 Z')`;


  return (
    <div
      className={`panel ${expanded ? "panel--open" : ""}`}
      onMouseEnter={expandPanel}
      onMouseLeave={collapsePanel}
      style={{ clipPath: panelClip }}
    >
      <div className="compact-header">
        <div className="wing wing--left">
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
        <div className="instances" ref={instancesRef}>
          <div className="instances-inner" ref={innerRef}>
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
                onClick={() => jumpToTerminal(inst.id)}
                onRespond={(choice) => respondPermission(inst.id, choice)}
                onSubagentClick={(id) => jumpToTerminal(id)}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
