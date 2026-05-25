// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import Button from "./Button";
import { ipcTestAutoPaste } from "../lib/tauri";
import { useT } from "../i18n";

const DEFAULT_TEXT = "VoiceTypeX Auto-Paste-Test";
const DELAY_SECS = 3;

/**
 * Diagnostic test for the auto-paste path. A click starts a
 * 3-second countdown — the user has time to focus the target
 * window. Separates the libei typing from the pipeline focus race.
 */
export default function AutoPasteTestSection() {
  const t = useT();
  const [running, setRunning] = useState(false);
  const [countdown, setCountdown] = useState(0);
  const [lastResult, setLastResult] = useState<string | null>(null);
  const [text, setText] = useState(DEFAULT_TEXT);

  const runTest = async () => {
    setRunning(true);
    setLastResult(null);
    try {
      for (let i = DELAY_SECS; i > 0; i--) {
        setCountdown(i);
        await new Promise((r) => setTimeout(r, 1000));
      }
      setCountdown(0);
      await ipcTestAutoPaste(text, 0);
      setLastResult(t("autopaste.status.success"));
    } catch (e) {
      setLastResult(t("common.error_prefix", { message: String(e) }));
    } finally {
      setRunning(false);
    }
  };

  const btnLabel = running
    ? countdown > 0
      ? t("autopaste.btn.countdown", { seconds: countdown })
      : t("autopaste.btn.injecting")
    : t("autopaste.btn.idle");

  return (
    <div className="rounded-md border border-outline bg-surface p-4">
      <h3 className="text-base font-semibold text-fg mb-2">
        {t("autopaste.title")}
      </h3>
      <p className="text-sm text-fg-muted mb-3">
        {t("autopaste.intro", { seconds: DELAY_SECS })}
      </p>
      <div className="flex flex-col gap-2">
        <input
          className="bg-elevated border border-outline rounded-md px-2 py-1.5 text-sm font-mono text-fg placeholder:text-fg-faint focus:outline-none focus:border-brand focus:ring-1 focus:ring-brand/40"
          value={text}
          onChange={(e) => setText(e.target.value)}
          disabled={running}
        />
        <Button
          onClick={runTest}
          disabled={running || text.trim().length === 0}
          className="self-start"
        >
          {btnLabel}
        </Button>
        {lastResult && (
          <div className="text-xs text-fg-muted mt-1">{lastResult}</div>
        )}
      </div>
    </div>
  );
}
