// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
 * Live-Overlay-Fenster. Folgt der Pipeline-State-Machine via
 * `app://state` und zeigt phasengerechte Status-Texte:
 *   recording      -> "Hoere zu …"  (roter Punkt pulsiert)
 *   transcribing   -> "Transkribiere …"
 *   postprocessing -> "Verarbeite …"
 *   injecting      -> "Fuege ein …"
 *   idle           -> Fade-out nach 800 ms
 *   error          -> Fehlertext, 2.5 s sichtbar
 */
export default function Overlay() {
  const [phase, setPhase] = useState<StatePayload["state"]>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const hideTimerRef = useRef<number | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];

    const showWindow = async () => {
      try {
        await getCurrentWindow().show();
      } catch {
        // Bereits sichtbar / nicht verfuegbar — beides harmlos.
      }
    };
    const hideWindow = async () => {
      try {
        await getCurrentWindow().hide();
      } catch {
        // Beim Shutdown evtl. weg.
      }
    };
    const cancelHide = () => {
      if (hideTimerRef.current !== null) {
        window.clearTimeout(hideTimerRef.current);
        hideTimerRef.current = null;
      }
    };

    listen<StatePayload>("app://state", (event) => {
      const next = event.payload.state;
      setPhase(next);
      setErrorMsg(event.payload.error ?? null);

      // DIAGNOSE-MODUS: Overlay-Show temporaer deaktiviert, weil
      // Verdacht besteht, dass das Overlay-Window beim show() den
      // Fokus klaut und libei-Strg+V damit ins Overlay statt in die
      // Ziel-App tippt. Wenn Auto-Paste mit deaktiviertem Overlay
      // funktioniert, ist Overlay der Schuldige und wir bauen einen
      // richtigen Fokus-neutralen Show-Pfad.
      const OVERLAY_SHOW_DISABLED_FOR_DIAGNOSIS = true;

      if (next === "idle" || next === "error") {
        cancelHide();
        hideTimerRef.current = window.setTimeout(
          () => {
            void hideWindow();
            hideTimerRef.current = null;
          },
          next === "error" ? 2500 : 800,
        );
      } else if (!OVERLAY_SHOW_DISABLED_FOR_DIAGNOSIS) {
        cancelHide();
        void showWindow();
      }
    }).then((u) => unlistens.push(u));

    return () => {
      cancelHide();
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
        return "";
    }
  })();

  const dotAnim = phase === "recording" ? "animate-pulse" : "";

  return (
    <div className="h-screen w-screen overflow-hidden p-2 select-none">
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
