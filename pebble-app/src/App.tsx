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
  details?: string;
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
  conversation_log?: string[];
  session_start?: number;
  transcript_path?: string;
  session_name?: string;
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

function getSessionName(inst: Instance): string {
  if (inst.session_name) {
    return inst.session_name;
  }
  // Prefer working directory name over transcript UUID fallback
  const cwdName = inst.working_directory.split(/[/\\]/).pop();
  if (cwdName && cwdName.length > 0) {
    return cwdName;
  }
  if (inst.transcript_path) {
    const parts = inst.transcript_path.split(/[/\\]/);
    const idx = parts.indexOf("transcripts");
    if (idx >= 0 && idx + 1 < parts.length && !/^[0-9a-f-]{36}$/i.test(parts[idx + 1])) {
      return parts[idx + 1];
    }
    const fileName = parts[parts.length - 1] || "";
    const slug = fileName.replace(/\.jsonl?$/, "");
    if (slug && slug.length > 0 && !/^[0-9a-f-]{36}$/i.test(slug)) {
      return slug;
    }
  }
  return inst.working_directory;
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
    return `Claude: Using ${ev.tool_name}`;
  }
  if (ev.event === "UserPromptSubmit") {
    return null; // handled in user message row
  }
  if (ev.event === "PostToolUse") {
    return `Claude: ${ev.tool_name || "Tool"} completed`;
  }
  if (ev.event === "PostToolUseFailure") {
    return `Claude: ${ev.tool_name || "Tool"} failed`;
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

function PixelGrid({
  pixels,
  color,
  size = 2,
  chars,
}: {
  pixels: string[];
  color: string;
  size?: number;
  chars?: string;
}) {
  const w = pixels[0]?.length ?? 0;
  const h = pixels.length;
  return (
    <svg width={w * size} height={h * size} style={{ display: "block" }}>
      {pixels.map((row, y) =>
        row.split("").map((char, x) => {
          if (char === " " || char === ".") return null;
          if (chars && !chars.includes(char)) return null;
          return <rect key={`${x}-${y}`} x={x * size} y={y * size} width={size} height={size} fill={color} />;
        })
      )}
    </svg>
  );
}

const CAPY_SM = [
  "  ee    ee  ",
  " bbbbbbbbbb ",
  "bo bbbbbb ob",
  "bbbbbbbbbbbb",
  "bbbbbbbbbbbb",
  " bbbbbbbbbb ",
  "  bbbbbbbb  ",
  " l l ll l l ",
];

const CAPY_MD = [
  "  ee    ee  ",
  " bbbbbbbbbb ",
  "bo bbbbbb ob",
  "bbbbbbbbbbbb",
  "bbbbbbbbbbbb",
  "bbbbbbbbbbbb",
  " bbbbbbbbbb ",
  "  bbbbbbbb  ",
  " l l ll l l ",
  " l l ll l l ",
];


function CapyPixel({ status, size }: { status: string; size: "sm" | "md" }) {
  const isAlert = status === "needs_permission";
  const isExecuting = status === "executing";
  const isCompleted = status === "completed";

  const wrapClass =
    `capy-wrap capy-wrap--${size} ` +
    (isAlert ? "capy-wrap--alert" : isExecuting ? "capy-wrap--executing" : isCompleted ? "capy-wrap--completed" : "capy-wrap--waiting");

  const pixels = size === "sm" ? CAPY_SM : CAPY_MD;
  const pixelSize = 2;

  const w = (pixels[0]?.length ?? 0) * pixelSize;
  const h = pixels.length * pixelSize;

  return (
    <div className={wrapClass}>
      <div className="capy-sprite" style={{ width: w, height: h, position: "relative" }}>
        <div className="capy-layer capy-body">
          <PixelGrid pixels={pixels} color="currentColor" size={pixelSize} chars="b" />
        </div>
        <div className="capy-layer capy-ears">
          <PixelGrid pixels={pixels} color="currentColor" size={pixelSize} chars="e" />
        </div>
        <div className="capy-layer capy-eyes">
          <PixelGrid pixels={pixels} color="#1a1625" size={pixelSize} chars="o" />
        </div>
        <div className="capy-layer capy-legs">
          <PixelGrid pixels={pixels} color="currentColor" size={pixelSize} chars="l" />
        </div>
      </div>
    </div>
  );
}

function MiniPet({ status }: { status: string }) {
  return <CapyPixel status={status} size="md" />;
}

function InstanceCard({
  inst,
  onClick,
  onSubagentClick,
}: {
  inst: Instance;
  onClick: () => void;
  onSubagentClick?: () => void;
}) {
  const [expandedSubagents, setExpandedSubagents] = useState(false);

  const displayPreview = useMemo(() => {
    if (inst.conversation_log && inst.conversation_log.length > 0) {
      return inst.conversation_log.join("\n");
    }
    const lines: string[] = [];
    const msg = getUserMessage(inst);
    if (msg) lines.push(msg);
    const action = getCurrentAction(inst);
    if (action) lines.push(action);
    if (lines.length > 0) return lines.join("\n");
    return "No recent activity";
  }, [inst]);

  const sessionName = getSessionName(inst);
  const runtime = inst.session_start != null ? formatDuration(inst.session_start) : formatTimeAgo(inst.last_activity);

  return (
    <div className={`instance instance--${inst.status}`}>
      <div className="instance-main" onClick={onClick}>
        <MiniPet status={inst.status} />
        <div className="instance-content">
          <div className="instance-title-row">
            <span className="instance-dir" title={inst.working_directory}>
              {sessionName}
            </span>
            <div className="instance-badges">
              <span className="badge badge--agent">{inst.model || "Claude"}</span>
              {inst.permission_mode && (
                <span className="badge badge--mode">{inst.permission_mode}</span>
              )}
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
          <div className="permission-title">
            {inst.pending_permission.tool_name} 请求
          </div>
          {inst.pending_permission.details && (
            <div className="permission-details" title={inst.pending_permission.details}>
              {inst.pending_permission.details}
            </div>
          )}
          <div className="permission-hint">
            请在终端中处理此请求
          </div>
        </div>
      )}

      {inst.subagents.length > 0 && (
        <div className="subagents">
          <div
            className="subagents-title subagents-title--clickable"
            onClick={(e) => {
              e.stopPropagation();
              setExpandedSubagents((v) => !v);
            }}
            title={inst.subagents.map((s) => s.name).join("; ")}
          >
            <span>Subagents ({inst.subagents.length})</span>
            <span className="subagents-chevron">{expandedSubagents ? "▲" : "▼"}</span>
          </div>
          {expandedSubagents && (
            <div className="subagents-list">
              {inst.subagents.map((sub) => {
                const fullText = sub.name;
                return (
                  <div
                    key={sub.id}
                    className={`subagent subagent--${sub.status}`}
                    onClick={() => onSubagentClick?.()}
                    title={fullText}
                  >
                    <StatusDot status={sub.status} />
                    <span className="subagent-name">{sub.name}</span>
                    <span className="subagent-status">{sub.status === "completed" ? "Done" : sub.status}</span>
                  </div>
                );
              })}
            </div>
          )}
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
  const resizeDebounceTimer = useRef<number | null>(null);
  const prevExpandedRef = useRef(false);
  const prevPermissionCountRef = useRef(0);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      e.preventDefault();
    };
    document.addEventListener("contextmenu", handler);
    return () => {
      document.removeEventListener("contextmenu", handler);
    };
  }, []);

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
    let mounted = true;
    (async () => {
      unlisten = await listen<Instance[]>("instances-updated", (e) => {
        if (!mounted) return;
        setInstances(e.payload);
        const permissionCount = e.payload.filter((i) => i.status === "needs_permission").length;
        const hadPermission = prevPermissionCountRef.current > 0;
        if (permissionCount > 0 && !hadPermission && !expanded) {
          expandPanelRef.current();
        }
        prevPermissionCountRef.current = permissionCount;
      });
    })();

    return () => {
      mounted = false;
      clearInterval(interval);
      if (unlisten) unlisten();
    };
  }, [expanded]);

  const expandPanelRef = useRef<() => void>(() => {});
  const collapsePanelRef = useRef<() => void>(() => {});

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let mounted = true;
    (async () => {
      unlisten = await listen<boolean>("pebble-hover", (e) => {
        if (!mounted) return;
        if (e.payload) {
          expandPanelRef.current();
        } else {
          collapsePanelRef.current();
        }
      });
    })();
    return () => {
      mounted = false;
      if (unlisten) unlisten();
    };
  }, []);

  const realInstances = useMemo(() => {
    const statusOrder: Record<string, number> = {
      needs_permission: 0,
      executing: 1,
      waiting: 2,
      completed: 3,
    };
    return instances
      .filter((i) => i.pid !== 0 || (i.last_activity > 0 && !!i.last_hook_event))
      .map((i) => ({ ...i, subagents: i.subagents || [] }))
      .sort((a, b) => {
        const pa = statusOrder[a.status] ?? 99;
        const pb = statusOrder[b.status] ?? 99;
        if (pa !== pb) return pa - pb;
        return a.working_directory.localeCompare(b.working_directory);
      });
  }, [instances]);
  const executingCount = realInstances.filter((i) => i.status === "executing").length;
  const permissionCount = realInstances.filter((i) => i.status === "needs_permission").length;
  const petMod = useMemo(() => {
    if (permissionCount > 0) return "needs_permission";
    if (executingCount > 0) return "executing";
    if (realInstances.some((i) => i.status === "waiting")) return "waiting";
    if (realInstances.some((i) => i.status === "completed")) return "completed";
    return "waiting";
  }, [permissionCount, executingCount, realInstances]);

  const instancesRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLDivElement>(null);
  const [desiredBodyH, setDesiredBodyH] = useState(COLLAPSED_H - FILLET_R);
  const lastHeightRef = useRef<number | null>(null);

  useLayoutEffect(() => {
    if (!expanded || !drawerVisible) return;
    const isFirstExpand = expanded && !prevExpandedRef.current;
    prevExpandedRef.current = expanded;

    const doResize = () => {
      const headerH = 38;
      const paddingB = 12;
      const contentHeight = innerRef.current?.offsetHeight ?? 0;
      const contentH = contentHeight + headerH + paddingB;
      const h = Math.min(Math.max(contentH, COLLAPSED_H - FILLET_R), MAX_EXPANDED_BODY_H);
      if (lastHeightRef.current !== h) {
        lastHeightRef.current = h;
        setDesiredBodyH(h);
        invoke("resize_window_centered", { width: EXPANDED_W, height: h + FILLET_R, animate: false }).catch(console.error);
      }
    };

    if (isFirstExpand) {
      if (resizeDebounceTimer.current) {
        window.clearTimeout(resizeDebounceTimer.current);
        resizeDebounceTimer.current = null;
      }
      lastHeightRef.current = null;
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
  }, [expanded, drawerVisible, realInstances]);

  useEffect(() => {
    if (!expanded || !drawerVisible) return;
    const ro = new ResizeObserver(() => {
      if (resizeDebounceTimer.current) {
        window.clearTimeout(resizeDebounceTimer.current);
      }
      resizeDebounceTimer.current = window.setTimeout(() => {
        const headerH = 38;
        const paddingB = 12;
        const contentHeight = innerRef.current?.offsetHeight ?? 0;
        const contentH = contentHeight + headerH + paddingB;
        const h = Math.min(Math.max(contentH, COLLAPSED_H - FILLET_R), MAX_EXPANDED_BODY_H);
        if (lastHeightRef.current !== h) {
          lastHeightRef.current = h;
          setDesiredBodyH(h);
          invoke("resize_window_centered", { width: EXPANDED_W, height: h + FILLET_R, animate: false }).catch(console.error);
        }
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
  }, [expanded, drawerVisible]);

  const expandPanel = () => {
    if (collapseTimer.current) {
      window.clearTimeout(collapseTimer.current);
      collapseTimer.current = null;
    }
    if (!expanded) {
      setExpanded(true);
      setDrawerVisible(true);
      invoke("bring_to_front").catch(console.error);
    } else if (!drawerVisible) {
      setDrawerVisible(true);
      invoke("bring_to_front").catch(console.error);
    }
  };

  const collapsePanel = () => {
    setDrawerVisible(false);
    if (collapseTimer.current) {
      window.clearTimeout(collapseTimer.current);
    }
    collapseTimer.current = window.setTimeout(() => {
      setExpanded(false);
      prevExpandedRef.current = false;
      lastHeightRef.current = null;
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
          <CapyPixel status={petMod} size="sm" />
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
                <div className="empty-sub">Start a session in your terminal</div>
              </div>
            )}
            {realInstances.map((inst) => (
              <InstanceCard
                key={inst.id}
                inst={inst}
                onClick={() => jumpToTerminal(inst.id)}
                onSubagentClick={() => jumpToTerminal(inst.id)}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
