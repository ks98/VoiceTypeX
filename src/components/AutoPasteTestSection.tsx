// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import { ipcTestAutoPaste } from "../lib/tauri";

const DEFAULT_TEXT = "VoiceTypeX Auto-Paste-Test";
const DELAY_SECS = 3;

/**
 * Diagnose-Test fuer den Auto-Paste-Pfad. Klick startet einen
 * 3-Sekunden-Countdown — der User hat Zeit, das Ziel-Fenster zu
 * fokussieren. Trennt das libei-Tippen vom Pipeline-Fokus-Race.
 */
export default function AutoPasteTestSection() {
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
      setLastResult("Inject ausgeloest — Text sollte im Ziel-Fenster sein.");
    } catch (e) {
      setLastResult(`Fehler: ${e}`);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="rounded-md border border-outline bg-surface p-4">
      <h3 className="text-base font-semibold text-fg mb-2">
        Auto-Paste-Test (Diagnose)
      </h3>
      <p className="text-sm text-fg-muted mb-3">
        Klick startet einen {DELAY_SECS}-Sekunden-Countdown. Fokussiere in
        dieser Zeit das Ziel-Fenster (Kate, Notepad, Browser-Adressleiste,
        …). Danach wird der Text via Clipboard + Strg+V eingefügt — ohne
        Audio-/STT-/LLM-Pipeline.
      </p>
      <div className="flex flex-col gap-2">
        <input
          className="bg-elevated border border-outline rounded-md px-2 py-1.5 text-sm font-mono text-fg placeholder:text-fg-faint focus:outline-none focus:border-brand focus:ring-1 focus:ring-brand/40"
          value={text}
          onChange={(e) => setText(e.target.value)}
          disabled={running}
        />
        <button
          onClick={runTest}
          disabled={running || text.trim().length === 0}
          className="inline-flex items-center self-start px-3 py-1.5 rounded-md bg-brand text-brand-contrast text-sm font-medium hover:bg-brand-hover transition-colors disabled:bg-elevated disabled:text-fg-faint disabled:cursor-not-allowed"
        >
          {running
            ? countdown > 0
              ? `Tippe in ${countdown} s … (jetzt Ziel-Fenster fokussieren)`
              : "Inject läuft …"
            : "Auto-Paste testen"}
        </button>
        {lastResult && (
          <div className="text-xs text-fg-muted mt-1">{lastResult}</div>
        )}
      </div>
    </div>
  );
}
