import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface Instance {
  id: string;
  pid: number;
  status: "waiting" | "executing" | "completed" | string;
  working_directory: string;
  terminal_app: string;
  last_activity: number;
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

  const realInstances = instances.filter((i) => i.pid !== 0);
  const executingCount = realInstances.filter((i) => i.status === "executing").length;

  return (
    <div className="panel">
      <div className="header">
        <h1 className="title">Pebble</h1>
        <div className="badge">
          {executingCount > 0 ? `${executingCount} active` : `${realInstances.length} instances`}
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
              <span className="instance-status">{inst.status}</span>
              <span className="instance-pid">PID {inst.pid || "—"}</span>
            </div>
            <div className="instance-dir">{inst.working_directory}</div>
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
        </div>
      </div>
    </div>
  );
}

export default App;
