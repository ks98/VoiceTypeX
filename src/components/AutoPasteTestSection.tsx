// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import { ipcTestAutoPaste } from "../lib/tauri";

const DEFAULT_TEXT = "VoiceTypeX Auto-Paste-Test";
const DELAY_SECS = 3;

/**
 * Diagnose-Test fuer den Auto-Paste-Pfad. Klick startet einen
 * 3-Sekunden-Countdown — der User hat Zeit, das Ziel-Fenster (z.B.
 * Kate, Browser-Adressleiste) zu fokussieren. Danach triggert die App
 * direkt einen Inject ohne Pipeline-Drumherum (Audio/STT/LLM).
 *
 * Sinn: trennt das libei-Tippen vom Pipeline-Fokus-Race. Wenn dieser
 * Test erfolgreich ist, der echte Hotkey-Pfad aber nicht, liegt das
 * Problem an dem, was zwischen Hotkey-Press und Inject den Fokus
 * verschiebt — nicht an libei selbst.
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
      // Counter im UI laufen lassen (kosmetisch).
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
    <div className="rounded-lg border border-slate-700 bg-slate-900/40 p-4 mt-4">
      <h3 className="text-base font-semibold text-slate-100 mb-2">
        Auto-Paste-Test (Diagnose)
      </h3>
      <p className="text-sm text-slate-400 mb-3">
        Klick startet einen {DELAY_SECS}-Sekunden-Countdown. Fokussiere in
        dieser Zeit das Ziel-Fenster (Kate, Notepad, Browser-Adressleiste,
        …). Danach wird der Text via Clipboard + Strg+V eingefügt — ohne
        Audio-/STT-/LLM-Pipeline.
      </p>
      <div className="flex flex-col gap-2">
        <input
          className="bg-slate-800 border border-slate-700 rounded px-2 py-1.5 text-sm font-mono text-slate-100"
          value={text}
          onChange={(e) => setText(e.target.value)}
          disabled={running}
        />
        <button
          onClick={runTest}
          disabled={running || text.trim().length === 0}
          className="bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-700 disabled:text-slate-400 text-white rounded px-3 py-1.5 text-sm font-medium transition"
        >
          {running
            ? countdown > 0
              ? `Tippe in ${countdown} s … (jetzt Ziel-Fenster fokussieren)`
              : "Inject läuft …"
            : "Auto-Paste testen"}
        </button>
        {lastResult && (
          <div className="text-xs text-slate-300 mt-1">{lastResult}</div>
        )}
      </div>
    </div>
  );
}
