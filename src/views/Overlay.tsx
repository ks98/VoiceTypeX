// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";

type StatePayload = {
  state:
    | "idle"
    | "recording"
    | "transcribing"
    | "postprocessing"
    | "injecting"
    | "error";
  error?: string;
};

/**
 * Live-Overlay-Fenster — zeigt waehrend Recording/Transcribe/… den
 * Pipeline-Phasen-Indikator.
 *
 * Sichtbarkeit wird **vom Backend** ueber `webview_window.show()` /
 * `webview_window.hide()` gesteuert (siehe `pipeline/mod.rs`):
 *   - `start_recording` → `overlay.show()`
 *   - direkt vor libei-Inject → `overlay.hide()` + 80 ms Pause, damit der
 *     Tastatur-Fokus zur Ziel-App zurueckspringt
 *   - State-Listener → bei Idle / Error → `overlay.hide()` (Cleanup)
 *
 * Die Modus-Auswahl-UI laeuft im separaten `menu`-Window (Menu.tsx) —
 * dieses Overlay hat keine Tastatur-Interaktion und kein Pointer-Events
 * (CSS-Schutz, falls es mal unerwartet sichtbar bleibt).
 *
 * Phase-Default ist `"recording"`, weil das Window vom Backend nur
 * sichtbar gemacht wird, *wenn* die Pipeline gerade in Recording wechselt.
 * Damit ist der erste sichtbare Frame schon "Höre zu …" statt leer.
 */
export default function Overlay() {
  const [phase, setPhase] = useState<StatePayload["state"]>("recording");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    listen<StatePayload>("app://state", (event) => {
      setPhase(event.payload.state);
      setErrorMsg(event.payload.error ?? null);
    }).then((u) => unlistens.push(u));

    return () => {
      unlistens.forEach((u) => u());
    };
  }, []);

  const phaseLabel = (() => {
    switch (phase) {
      case "recording":
        return "Höre zu …";
      case "transcribing":
        return "Transkribiere …";
      case "postprocessing":
        return "Verarbeite …";
      case "injecting":
        return "Füge ein …";
      case "error":
        return errorMsg ? `Fehler: ${errorMsg}` : "Fehler";
      default:
        return "Höre zu …";
    }
  })();

  const dotAnim = phase === "recording" ? "animate-pulse" : "";

  return (
    <div className="h-screen w-screen overflow-hidden p-2 select-none pointer-events-none">
      <div className="h-full w-full rounded-lg bg-black/75 backdrop-blur-md border border-white/10 shadow-2xl px-4 py-3 flex items-center">
        <div className="flex items-center gap-3 w-full min-w-0">
          <span
            className={`inline-block h-2.5 w-2.5 rounded-full shrink-0 bg-red-500 ${dotAnim}`}
            aria-hidden
          />
          <p className="text-white text-sm leading-snug font-medium overflow-hidden text-ellipsis whitespace-nowrap">
            <span className="text-white/80 italic">{phaseLabel}</span>
          </p>
        </div>
      </div>
    </div>
  );
}
