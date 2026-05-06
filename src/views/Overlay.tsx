// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useRef, useState } from "react";
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
 * Live-Overlay-Fenster.
 *
 * **Wichtige Architektur-Entscheidung:** Das Overlay-Fenster ist
 * dauerhaft sichtbar (`visible: true` in tauri.conf.json), wir togglen
 * NUR die CSS-Opacity zwischen 0 und 1. Grund: ein `getCurrentWindow().show()`
 * auf Wayland (KDE Plasma) klaut den Tastatur-Fokus, auch wenn `focus: false`
 * in der Window-Config steht. Damit landeten libei-Strg+V-Events im
 * Overlay statt in der Ziel-App. Per Opacity wird kein Window-Aktivierungs-
 * Event erzeugt, der Fokus bleibt bei der Ziel-App.
 *
 * Damit der dauerhaft sichtbare Window nicht Klicks abfängt: in `lib.rs`
 * wird `set_ignore_cursor_events(true)` aufgerufen.
 */
export default function Overlay() {
  const [phase, setPhase] = useState<StatePayload["state"]>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const hideTimerRef = useRef<number | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];

    const cancelHide = () => {
      if (hideTimerRef.current !== null) {
        window.clearTimeout(hideTimerRef.current);
        hideTimerRef.current = null;
      }
    };

    listen<StatePayload>("app://state", (event) => {
      const next = event.payload.state;
      setErrorMsg(event.payload.error ?? null);
      cancelHide();

      if (next === "idle" || next === "error") {
        // Phase-Wert bleibt kurz auf dem alten Wert stehen, damit der
        // letzte Status (z.B. Fehlertext) beim Fade-out noch lesbar ist.
        // Erst nach dem Timeout wechseln wir auf 'idle' — damit wird die
        // Opacity 0 und das Overlay verschwindet visuell.
        hideTimerRef.current = window.setTimeout(
          () => {
            setPhase("idle");
            hideTimerRef.current = null;
          },
          next === "error" ? 2500 : 800,
        );
        // Bei error trotzdem den Fehler-Phase setzen, damit der Text
        // sichtbar wird waehrend des 2.5s-Timeouts.
        if (next === "error") {
          setPhase("error");
        }
      } else {
        setPhase(next);
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

  const isVisible = phase !== "idle";
  const dotAnim = phase === "recording" ? "animate-pulse" : "";

  return (
    <div
      className={`h-screen w-screen overflow-hidden p-2 select-none pointer-events-none transition-opacity duration-200 ${
        isVisible ? "opacity-100" : "opacity-0"
      }`}
    >
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
