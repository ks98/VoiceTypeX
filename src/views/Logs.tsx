// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { ipcGetRecentLogs } from "../lib/tauri";

export default function Logs(): JSX.Element {
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const fresh = await ipcGetRecentLogs(200);
        if (!cancelled) {
          setLines(fresh);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setError(String(e));
        }
      }
    };
    void tick();
    const interval = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, []);

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-slate-400">
        Echtzeit-Log-Stream wird in Phase 1.6 ueber einen Tracing-Ringbuffer und
        ein <code>log_line</code>-Event verdrahtet. Aktuell holt das UI die
        letzten Eintraege per Pull alle 2 Sekunden.
      </p>
      {error ? (
        <div className="rounded-md bg-red-900/30 border border-red-700 px-3 py-2 text-sm text-red-300">
          {error}
        </div>
      ) : null}
      <pre className="bg-slate-900 border border-slate-800 rounded p-3 text-xs font-mono text-slate-300 overflow-auto max-h-[60vh]">
        {lines.length === 0 ? "(noch keine Logs)" : lines.join("\n")}
      </pre>
    </div>
  );
}
