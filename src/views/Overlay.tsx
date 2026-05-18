// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";

type Phase =
  | "idle"
  | "recording"
  | "transcribing"
  | "postprocessing"
  | "injecting"
  | "error";

type StatePayload = { state: Phase; error?: string };
type PartialTranscriptPayload = { text: string };

/**
 * Live-Overlay-Fenster — zeigt während Recording/Transcribe/… den
 * Pipeline-Phasen-Indikator. Sichtbarkeit wird vom Backend gesteuert.
 *
 * Phase-Default ist "recording", weil das Window vom Backend nur
 * sichtbar gemacht wird, wenn die Pipeline gerade in Recording wechselt.
 * Damit ist der erste sichtbare Frame schon "Höre zu …" statt leer.
 *
 * Phase 2: Während Recording mit lokalem STT emittiert das Backend
 * `app://partial-transcript`-Events mit stabilen Wort-Prefixen aus
 * LocalAgreement-2. Wir zeigen den jeweils letzten Stand in einer
 * zweiten Zeile unter dem Status-Header. Bei Phasen-Wechsel weg von
 * Recording (Transcribing/Postprocessing/Injecting/Idle) wird der
 * Partial implizit geleert — entweder durch einen leeren Event vom
 * Backend oder durch unseren lokalen Phase-Reset.
 */
export default function Overlay(): JSX.Element {
  const [phase, setPhase] = useState<Phase>("recording");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [partial, setPartial] = useState<string>("");

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    listen<StatePayload>("app://state", (event) => {
      setPhase(event.payload.state);
      setErrorMsg(event.payload.error ?? null);
      // Phase verlaesst Recording → Partial loeschen, damit der naechste
      // Recording-Zyklus mit leerer Anzeige startet.
      if (event.payload.state !== "recording") {
        setPartial("");
      }
    }).then((u) => unlistens.push(u));

    listen<PartialTranscriptPayload>("app://partial-transcript", (event) => {
      setPartial(event.payload.text ?? "");
    }).then((u) => unlistens.push(u));

    return () => {
      unlistens.forEach((u) => u());
    };
  }, []);

  const meta = phaseMeta(phase, errorMsg);
  const visiblePartial = truncateStart(partial, 65);

  return (
    <div className="h-screen w-screen overflow-hidden p-2 select-none pointer-events-none">
      <div className="h-full w-full rounded-lg bg-canvas/80 backdrop-blur-xl border border-fg/10 shadow-2xl px-4 py-2.5 flex flex-col justify-center gap-1">
        <div className="flex items-center gap-3">
          <span
            className={`shrink-0 inline-flex items-center justify-center h-7 w-7 rounded-md ${meta.iconBg} ${meta.iconColor}`}
            aria-hidden
          >
            {meta.icon}
          </span>
          <p className="flex-1 text-fg text-sm leading-snug font-medium overflow-hidden text-ellipsis whitespace-nowrap">
            {meta.label}
          </p>
          {phase === "recording" ? <Equalizer /> : null}
        </div>
        {phase === "recording" && visiblePartial ? (
          <p
            className="text-fg/60 text-xs leading-snug pl-10 overflow-hidden text-ellipsis whitespace-nowrap"
            title={partial}
          >
            {visiblePartial}
          </p>
        ) : null}
      </div>
    </div>
  );
}

/**
 * Wenn der Text laenger als `max` ist, zeig nur das Ende mit
 * Ellipsis am Anfang — fuehlt sich live an ("…was gerade gesagt wird"),
 * passt zur fest-breiten Overlay-Box ohne Umbruch.
 */
function truncateStart(text: string, max: number): string {
  if (!text) return "";
  if (text.length <= max) return text;
  return `…${text.slice(text.length - max)}`;
}

function Equalizer(): JSX.Element {
  const bar =
    "w-1 h-4 bg-status-recording rounded-sm origin-bottom animate-[vtx-eq_700ms_ease-in-out_infinite]";
  return (
    <div className="shrink-0 flex items-end gap-0.5 h-5 pr-1" aria-hidden>
      <span className={bar} />
      <span className={`${bar} [animation-delay:120ms]`} />
      <span className={`${bar} [animation-delay:240ms]`} />
    </div>
  );
}

interface PhaseMeta {
  label: string;
  icon: JSX.Element;
  iconBg: string;
  iconColor: string;
}

function phaseMeta(phase: Phase, errorMsg: string | null): PhaseMeta {
  switch (phase) {
    case "recording":
      return {
        label: "Höre zu …",
        icon: <MicIcon />,
        iconBg: "bg-status-recording/15",
        iconColor: "text-status-recording",
      };
    case "transcribing":
      return {
        label: "Transkribiere …",
        icon: <WaveIcon />,
        iconBg: "bg-brand/15",
        iconColor: "text-brand",
      };
    case "postprocessing":
      return {
        label: "Verarbeite …",
        icon: <SparkleIcon />,
        iconBg: "bg-status-processing/15",
        iconColor: "text-status-processing",
      };
    case "injecting":
      return {
        label: "Füge ein …",
        icon: <ArrowRightIcon />,
        iconBg: "bg-brand/15",
        iconColor: "text-brand",
      };
    case "error":
      return {
        label: errorMsg ? `Fehler: ${errorMsg}` : "Fehler",
        icon: <AlertIcon />,
        iconBg: "bg-status-error/15",
        iconColor: "text-status-error",
      };
    case "idle":
    default:
      return {
        label: "Höre zu …",
        icon: <MicIcon />,
        iconBg: "bg-status-recording/15",
        iconColor: "text-status-recording",
      };
  }
}

function MicIcon(): JSX.Element {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="3" width="6" height="12" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 19v3" />
    </svg>
  );
}

function WaveIcon(): JSX.Element {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 12h2l2-6 4 12 4-9 2 3h2" />
    </svg>
  );
}

function SparkleIcon(): JSX.Element {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M5.6 18.4l2.8-2.8M15.6 8.4l2.8-2.8" />
    </svg>
  );
}

function ArrowRightIcon(): JSX.Element {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M5 12h14M13 5l7 7-7 7" />
    </svg>
  );
}

function AlertIcon(): JSX.Element {
  return (
    <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9" />
      <path d="M12 8v5M12 16h.01" />
    </svg>
  );
}
