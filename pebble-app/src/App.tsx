import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

function App() {
  const [instances, setInstances] = useState<Instance[]>([]);

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
    <div className="panel">
      <div className="header">
        <h1 className="title">Pebble</h1>
        <div className="badge">
          {permissionCount > 0
            ? `${permissionCount} pending`
            : executingCount > 0
            ? `${executingCount} active`
            : `${realInstances.length} instances`}
        </div>
      </div>
      <div className="instances">
        {realInstances.length === 0 && (
          <div className="empty">
            No Claude Code instances found
            <div className="empty-sub">
              Start a Claude Code session in iTerm2 to see it here
            </div>
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
              <div className="permission-card" onClick={(e) => e.stopPropagation()}>
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
            <span className="status-dot status-dot--waiting" /> waiting
          </span>
          <span className="legend-item">
            <span className="status-dot status-dot--executing" /> executing
          </span>
          <span className="legend-item">
            <span className="status-dot status-dot--needs_permission" /> approval
          </span>
        </div>
      </div>
    </div>
  );
}

export default App;
