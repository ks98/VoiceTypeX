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
type PartialPayload = {
  mode_id: string;
  text: string;
  is_final: boolean;
};
type FinalPayload = {
  mode_id: string;
  text: string;
};

/**
 * Live-Overlay-Fenster.
 *
 * Zwei Datenquellen:
 *   1. `app://state` — folgt der Pipeline-State-Machine und steuert
 *      Sichtbarkeit + Status-Label ("Hoere zu …", "Verarbeite …", …).
 *      Damit ist das Overlay auch im One-Shot-Pfad nuetzlich.
 *   2. `stt://partial` / `stt://final` — Streaming-Live-Text. Wird
 *      ergaenzend zum State-Label angezeigt, wenn Streaming aktiv ist.
 */
/**
 * Suppression-Fenster fuer interim-Updates am Anfang einer Aufnahme.
 *
 * Hintergrund: xAI's STT-Server faengt bei kurzen Audio-Snippets oft mit
 * englischer Auto-Detect-Vermutung an und korrigiert auf Deutsch, sobald
 * mehr Audio da ist. Der Final-Text ist dann korrekt, aber das initiale
 * "Hello"-Phantom flackert ~1 s im Overlay. Loesung: in den ersten
 * INTERIM_SUPPRESS_MS nach Recording-Start zeigen wir nur "Hoere zu …",
 * keine interim-Updates. Bis dahin hat das Modell sich kalibriert.
 */
const INTERIM_SUPPRESS_MS = 1000;

export default function Overlay() {
  const [phase, setPhase] = useState<StatePayload["state"]>("idle");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [finals, setFinals] = useState<string>("");
  const [interim, setInterim] = useState<string>("");
  const hideTimerRef = useRef<number | null>(null);
  const recordingStartRef = useRef<number | null>(null);

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

      if (next === "recording") {
        cancelHide();
        setFinals("");
        setInterim("");
        recordingStartRef.current = Date.now();
        void showWindow();
      } else if (next === "idle" || next === "error") {
        // Kurzer Verbleib, damit der finale Text / Fehler noch wahrnehmbar ist.
        cancelHide();
        hideTimerRef.current = window.setTimeout(
          () => {
            setFinals("");
            setInterim("");
            void hideWindow();
            hideTimerRef.current = null;
          },
          next === "error" ? 2500 : 800,
        );
      } else {
        // transcribing / postprocessing / injecting → sichtbar bleiben
        cancelHide();
        void showWindow();
      }
    }).then((u) => unlistens.push(u));

    listen<PartialPayload>("stt://partial", (event) => {
      const { text, is_final } = event.payload;

      // Suppression-Fenster: interim-Updates der ersten ~Sekunde
      // verwerfen, weil xAI's auto-detect dort haeufig auf Englisch
      // raet, bevor das Modell genug Audio fuer eine stabile Sprach-
      // Entscheidung hat. Final-Tokens werden NICHT unterdrueckt —
      // die akkumulieren wir weiter, weil der Server sie ohnehin erst
      // nach kalibrierter Sprache ausgibt.
      const startedAt = recordingStartRef.current;
      const inSuppressionWindow =
        !is_final &&
        startedAt !== null &&
        Date.now() - startedAt < INTERIM_SUPPRESS_MS;

      if (is_final) {
        setFinals((prev) => (prev ? prev + " " + text : text));
        setInterim("");
      } else if (!inSuppressionWindow) {
        setInterim(text);
      }
    }).then((u) => unlistens.push(u));

    listen<FinalPayload>("stt://final", (event) => {
      setFinals(event.payload.text);
      setInterim("");
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

  const dotColor = phase === "error" ? "bg-red-500" : "bg-red-500";
  const dotAnim = phase === "recording" ? "animate-pulse" : "";

  const liveText = finals || interim;

  return (
    <div className="h-screen w-screen overflow-hidden p-2 select-none">
      <div className="h-full w-full rounded-lg bg-black/75 backdrop-blur-md border border-white/10 shadow-2xl px-4 py-3 flex items-center">
        <div className="flex items-center gap-3 w-full min-w-0">
          <span
            className={`inline-block h-2.5 w-2.5 rounded-full shrink-0 ${dotColor} ${dotAnim}`}
            aria-hidden
          />
          <p className="text-white text-sm leading-snug font-medium overflow-hidden text-ellipsis whitespace-nowrap">
            {liveText ? (
              <>
                <span className="text-white">{finals}</span>
                {interim && (
                  <>
                    {finals && " "}
                    <span className="text-white/60 italic">{interim}</span>
                  </>
                )}
              </>
            ) : (
              <span className="text-white/80 italic">{phaseLabel}</span>
            )}
          </p>
        </div>
      </div>
    </div>
  );
}
