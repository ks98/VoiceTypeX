// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef, useState } from "react";
import Banner from "../components/Banner";
import Button from "../components/Button";
import { ipcGetRecentLogs } from "../lib/tauri";

type LevelFilter = "all" | "warn" | "error";

// Log-Zeilen kommen aus dem Rust-Ringbuffer im Format
// `[LEVEL] target - message`, wobei LEVEL auf 5 Zeichen
// padded ist (z.B. `INFO `). Wir extrahieren das Level
// fuer den Filter direkt am Zeilenanfang.
function extractLevel(line: string): string | null {
  if (line.length < 7 || line[0] !== "[") return null;
  const close = line.indexOf("]");
  if (close < 0) return null;
  return line.slice(1, close).trim().toUpperCase();
}

function matchesFilter(line: string, filter: LevelFilter): boolean {
  if (filter === "all") return true;
  const level = extractLevel(line);
  if (filter === "error") return level === "ERROR";
  if (filter === "warn") return level === "WARN" || level === "ERROR";
  return true;
}

export default function Logs(): JSX.Element {
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [pauseSnapshot, setPauseSnapshot] = useState<string[]>([]);
  const [filter, setFilter] = useState<LevelFilter>("all");
  const [copyStatus, setCopyStatus] = useState<string | null>(null);

  const preRef = useRef<HTMLPreElement | null>(null);
  // Auto-Scroll-Heuristik: Vor jedem Append pruefen, ob User
  // noch am Ende klebt. Wir merken uns das *vor* dem Render-
  // Commit, damit der Scroll-Effect entscheiden kann, ob er
  // ans Ende springt oder die User-Position respektiert.
  const stickToBottomRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const fresh = await ipcGetRecentLogs(200);
        if (cancelled) return;
        const pre = preRef.current;
        if (pre) {
          stickToBottomRef.current =
            pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 30;
        }
        setLines(fresh);
        setError(null);
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

  // Was tatsaechlich angezeigt wird: bei Pause der eingefrorene
  // Snapshot, sonst der Live-Stream. Filter wird danach angewandt.
  const displayLines = useMemo(() => {
    const source = paused ? pauseSnapshot : lines;
    if (filter === "all") return source;
    return source.filter((line) => matchesFilter(line, filter));
  }, [paused, pauseSnapshot, lines, filter]);

  // Nach Render: ans Ende springen, wenn User vor dem Update
  // schon dort war. Greift auch beim Toggle Live → bewusster
  // Sprung ans Ende ist im UX-Briefing gefordert.
  useEffect(() => {
    if (paused) return;
    if (!stickToBottomRef.current) return;
    const pre = preRef.current;
    if (pre) {
      pre.scrollTop = pre.scrollHeight;
    }
  }, [displayLines, paused]);

  const missedCount = paused
    ? Math.max(0, lines.length - pauseSnapshot.length)
    : 0;

  const togglePause = (): void => {
    if (paused) {
      setPaused(false);
      // Beim Resume erzwingen wir „am Ende kleben", damit der
      // User die neuesten Eintraege sieht.
      stickToBottomRef.current = true;
    } else {
      setPauseSnapshot(lines);
      setPaused(true);
    }
  };

  const handleCopy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(displayLines.join("\n"));
      setCopyStatus("Kopiert");
    } catch (e) {
      setCopyStatus(`Fehler: ${String(e)}`);
    }
    window.setTimeout(() => setCopyStatus(null), 2000);
  };

  return (
    <div className="flex flex-col gap-3 h-full">
      <div className="flex items-center justify-between gap-3 flex-wrap">
        <div className="flex items-center gap-2">
          <Button
            variant={paused ? "secondary" : "ghost"}
            size="sm"
            onClick={togglePause}
            aria-pressed={paused}
          >
            {paused ? "Pausiert ⏸" : "Live ⏵"}
          </Button>
          {paused && missedCount > 0 ? (
            <span className="text-xs text-fg-faint">
              +{missedCount} neue waehrend Pause
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-2">
          <div role="group" aria-label="Level-Filter" className="flex gap-1">
            <Button
              variant={filter === "all" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setFilter("all")}
              aria-pressed={filter === "all"}
            >
              Alle
            </Button>
            <Button
              variant={filter === "warn" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setFilter("warn")}
              aria-pressed={filter === "warn"}
            >
              Warnungen
            </Button>
            <Button
              variant={filter === "error" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setFilter("error")}
              aria-pressed={filter === "error"}
            >
              Fehler
            </Button>
          </div>
          <Button variant="ghost" size="sm" onClick={() => void handleCopy()}>
            Kopieren
          </Button>
          {copyStatus ? (
            <span
              className="text-xs text-fg-faint"
              role="status"
              aria-live="polite"
            >
              {copyStatus}
            </span>
          ) : null}
        </div>
      </div>
      {error ? <Banner tone="error">{error}</Banner> : null}
      {displayLines.length === 0 ? (
        <Banner tone="info" dense>
          {filter === "all"
            ? "Noch keine Logs gesammelt."
            : "Keine Eintraege fuer diesen Filter."}
        </Banner>
      ) : null}
      <pre
        ref={preRef}
        role="log"
        aria-live="polite"
        aria-label="Log-Stream"
        className="bg-surface border border-outline rounded-md p-3 text-xs font-mono text-fg-muted overflow-auto flex-1 min-h-0"
      >
        {displayLines.join("\n")}
      </pre>
    </div>
  );
}
