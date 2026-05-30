// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef, useState } from "react";
import Banner from "../components/Banner";
import Button from "../components/Button";
import { ipcGetRecentLogs } from "../lib/tauri";
import { useT } from "../i18n";

type LevelFilter = "all" | "warn" | "error";

// Log lines come from the Rust ring buffer in the format
// `[LEVEL] target - message`, where LEVEL is padded to 5 chars
// (e.g. `INFO `). We extract the level for the filter directly at
// the start of the line.
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
  const t = useT();
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [pauseSnapshot, setPauseSnapshot] = useState<string[]>([]);
  const [filter, setFilter] = useState<LevelFilter>("all");
  const [copyStatus, setCopyStatus] = useState<string | null>(null);

  const preRef = useRef<HTMLPreElement | null>(null);
  // Auto-scroll heuristic: before every append, check whether the
  // user is still glued to the end. We record this *before* the
  // render commit, so the scroll effect can decide whether to jump
  // to the end or respect the user's position.
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

  // What's actually shown: the frozen snapshot when paused,
  // otherwise the live stream. Filter is applied after that.
  const displayLines = useMemo(() => {
    const source = paused ? pauseSnapshot : lines;
    if (filter === "all") return source;
    return source.filter((line) => matchesFilter(line, filter));
  }, [paused, pauseSnapshot, lines, filter]);

  // After render: jump to the end if the user was already there
  // before the update. This also fires on toggle → Live — a
  // deliberate jump to the end is required by the UX brief.
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
      // On resume we force "stick to the bottom" so the user sees
      // the newest entries.
      stickToBottomRef.current = true;
    } else {
      setPauseSnapshot(lines);
      setPaused(true);
    }
  };

  const handleCopy = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(displayLines.join("\n"));
      setCopyStatus(t("logs.copy.status_ok"));
    } catch (e) {
      setCopyStatus(t("common.error_prefix", { message: String(e) }));
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
            {paused ? t("logs.toggle.paused") : t("logs.toggle.live")}
          </Button>
          {paused && missedCount > 0 ? (
            <span className="text-xs text-fg-faint">
              {t("logs.missed", { count: missedCount })}
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-2">
          <div
            role="group"
            aria-label={t("logs.filter.group_label")}
            className="flex gap-1"
          >
            <Button
              variant={filter === "all" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setFilter("all")}
              aria-pressed={filter === "all"}
            >
              {t("logs.filter.all")}
            </Button>
            <Button
              variant={filter === "warn" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setFilter("warn")}
              aria-pressed={filter === "warn"}
            >
              {t("logs.filter.warn")}
            </Button>
            <Button
              variant={filter === "error" ? "secondary" : "ghost"}
              size="sm"
              onClick={() => setFilter("error")}
              aria-pressed={filter === "error"}
            >
              {t("logs.filter.error")}
            </Button>
          </div>
          <Button variant="ghost" size="sm" onClick={() => void handleCopy()}>
            {t("logs.copy.action")}
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
          {filter === "all" ? t("logs.empty.all") : t("logs.empty.filtered")}
        </Banner>
      ) : null}
      <pre
        ref={preRef}
        role="log"
        aria-live="polite"
        aria-label={t("logs.aria.stream")}
        className="bg-surface border border-outline rounded-md p-3 text-xs font-mono text-fg-muted overflow-auto flex-1 min-h-0"
      >
        {displayLines.join("\n")}
      </pre>
    </div>
  );
}
