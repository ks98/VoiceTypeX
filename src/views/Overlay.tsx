// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type PartialPayload = {
  mode_id: string;
  text: string;
  is_final: boolean;
};
type FinalPayload = {
  mode_id: string;
  text: string;
};
type DonePayload = {
  mode_id: string;
};

/**
 * Live-Overlay-Fenster: empfaengt Streaming-STT-Events vom Backend
 * (`stt://partial`, `stt://final`, `stt://done`) und zeigt den
 * akkumulierten Text waehrend des Sprechens. Beim `done`-Event
 * versteckt sich das Fenster wieder.
 *
 * Anzeigemodell:
 *   - "Final"-Segmente bleiben schwarz (akkumuliert).
 *   - Aktuelles "interim"-Segment wird grau angehaengt — verschwindet,
 *     sobald der Server es als final markiert (is_final=true).
 */
export default function Overlay() {
  const [finals, setFinals] = useState<string>("");
  const [interim, setInterim] = useState<string>("");
  const hideTimerRef = useRef<number | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];

    const showWindow = async () => {
      try {
        await getCurrentWindow().show();
      } catch {
        // Fenster bereits sichtbar oder nicht verfuegbar — beide harmlos.
      }
    };
    const hideWindow = async () => {
      try {
        await getCurrentWindow().hide();
      } catch {
        // Beim Shutdown kann das Fenster schon weg sein.
      }
    };

    const cancelHide = () => {
      if (hideTimerRef.current !== null) {
        window.clearTimeout(hideTimerRef.current);
        hideTimerRef.current = null;
      }
    };

    listen<PartialPayload>("stt://partial", (event) => {
      cancelHide();
      void showWindow();
      const { text, is_final } = event.payload;
      if (is_final) {
        setFinals((prev) => (prev ? prev + " " + text : text));
        setInterim("");
      } else {
        setInterim(text);
      }
    }).then((u) => unlistens.push(u));

    listen<FinalPayload>("stt://final", (event) => {
      cancelHide();
      const { text } = event.payload;
      setFinals(text);
      setInterim("");
    }).then((u) => unlistens.push(u));

    listen<DonePayload>("stt://done", () => {
      // Kurzer Verbleib, damit der finale Text noch wahrnehmbar ist.
      hideTimerRef.current = window.setTimeout(() => {
        setFinals("");
        setInterim("");
        void hideWindow();
        hideTimerRef.current = null;
      }, 800);
    }).then((u) => unlistens.push(u));

    return () => {
      cancelHide();
      unlistens.forEach((u) => u());
    };
  }, []);

  const hasContent = finals.length > 0 || interim.length > 0;

  return (
    <div className="h-screen w-screen overflow-hidden p-2 select-none">
      <div className="h-full w-full rounded-lg bg-black/75 backdrop-blur-md border border-white/10 shadow-2xl px-4 py-3 flex items-center">
        <div className="flex items-center gap-3 w-full">
          <span
            className="inline-block h-2.5 w-2.5 rounded-full bg-red-500 animate-pulse shrink-0"
            aria-hidden
          />
          <p className="text-white text-sm leading-snug font-medium truncate-3-lines">
            {hasContent ? (
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
              <span className="text-white/60 italic">Hoere zu …</span>
            )}
          </p>
        </div>
      </div>
    </div>
  );
}
