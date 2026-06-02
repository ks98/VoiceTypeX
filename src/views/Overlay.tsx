// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { listen, emit } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useT, type TranslateFn } from "../i18n";

type Phase =
  | "idle"
  | "recording"
  | "transcribing"
  | "postprocessing"
  | "injecting"
  | "error";

type StatePayload = { state: Phase; error?: string };
type PartialTranscriptPayload = { text: string };
type EngineSegment = {
  location: "local" | "cloud";
  provider: string | null;
  model: string;
};
type EngineStatusPayload = { stt: EngineSegment; llm: EngineSegment | null };

/**
 * Live overlay window — displays the pipeline-phase indicator during
 * recording/transcribe/… Visibility is driven by the backend.
 *
 * The phase default is "recording" because the backend only makes the
 * window visible when the pipeline transitions into recording. That way
 * the first visible frame already shows "Listening…" instead of being
 * empty.
 *
 * Phase 2: while recording with local STT, the backend emits
 * `app://partial-transcript` events carrying stable word prefixes from
 * LocalAgreement-2. We show the latest snapshot in a second line below
 * the status header. On a phase change away from recording
 * (transcribing/postprocessing/injecting/idle) the partial is cleared
 * implicitly — either by an empty event from the backend or by our
 * local phase reset.
 */
export default function Overlay(): JSX.Element {
  const t = useT();
  const [phase, setPhase] = useState<Phase>("recording");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [partial, setPartial] = useState<string>("");
  const [engine, setEngine] = useState<EngineStatusPayload | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    listen<StatePayload>("app://state", (event) => {
      setPhase(event.payload.state);
      setErrorMsg(event.payload.error ?? null);
      // Phase leaves recording → clear partial so the next recording
      // cycle starts with an empty display.
      if (event.payload.state !== "recording") {
        setPartial("");
      }
    }).then((u) => unlistens.push(u));

    listen<PartialTranscriptPayload>("app://partial-transcript", (event) => {
      setPartial(event.payload.text ?? "");
    }).then((u) => unlistens.push(u));

    // Engine status (issue #8): emitted by the backend when a mode becomes
    // active (recording start). Stays until the next recording overwrites it.
    listen<EngineStatusPayload>("app://active-engine", (event) => {
      setEngine(event.payload);
    }).then((u) => unlistens.push(u));

    return () => {
      unlistens.forEach((u) => u());
    };
  }, []);

  const meta = phaseMeta(t, phase, errorMsg);
  const visiblePartial = truncateStart(partial, 65);
  const isError = phase === "error";

  return (
    <div className="h-screen w-screen overflow-hidden p-2 select-none pointer-events-none">
      <div
        className={
          "h-full w-full rounded-lg vtx-glass shadow-2xl px-4 py-2.5 flex flex-col justify-center gap-1 " +
          // E1: same container, key-based content — cross-fade.
          // A4: on error, clicks must register (detail path).
          (isError ? "pointer-events-auto" : "")
        }
      >
        <div
          key={phase}
          className="flex items-center gap-3 animate-[vtx-fadein_200ms_ease-out] transition-opacity duration-200"
        >
          <span
            className={`shrink-0 inline-flex items-center justify-center h-7 w-7 rounded-md ${meta.iconBg} ${meta.iconColor}`}
            aria-hidden
          >
            {meta.icon}
          </span>
          {isError ? (
            <button
              type="button"
              onClick={openLogsInMainWindow}
              title={errorMsg ?? t("overlay.error_tooltip_fallback")}
              className="flex-1 text-left text-fg text-sm leading-snug font-medium whitespace-normal break-words cursor-pointer hover:text-status-error focus:outline-none focus-visible:ring-2 focus-visible:ring-status-error/50 rounded"
            >
              {meta.label}
            </button>
          ) : (
            <p className="flex-1 text-fg text-sm leading-snug font-medium overflow-hidden text-ellipsis whitespace-nowrap">
              {meta.label}
            </p>
          )}
          {phase === "recording" ? <RecordingDot /> : null}
        </div>
        {phase === "recording" && visiblePartial ? (
          <p
            key={visiblePartial}
            className="text-fg text-xs italic leading-snug pl-10 pr-1 overflow-hidden text-ellipsis whitespace-nowrap animate-[vtx-fadein_120ms_ease-out]"
            title={partial}
          >
            {visiblePartial}
          </p>
        ) : null}
        {engine && !isError ? (
          <div className="flex items-center gap-1.5 text-[10px] text-fg-faint pl-10 pr-1 overflow-hidden whitespace-nowrap">
            <EngineSeg label={t("overlay.engine.stt")} seg={engine.stt} t={t} />
            {engine.llm ? (
              <>
                <span className="text-outline" aria-hidden>
                  ·
                </span>
                <EngineSeg
                  label={t("overlay.engine.llm")}
                  seg={engine.llm}
                  t={t}
                />
              </>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}

/** One engine segment (STT or LLM) of the overlay status line: a label, a
 *  local/cloud dot, the location word, and `provider·model`. */
function EngineSeg({
  label,
  seg,
  t,
}: {
  label: string;
  seg: EngineSegment;
  t: TranslateFn;
}): JSX.Element {
  const isLocal = seg.location === "local";
  const detail = seg.provider
    ? seg.model
      ? `${seg.provider}·${seg.model}`
      : seg.provider
    : seg.model;
  return (
    <span className="inline-flex items-center gap-1 overflow-hidden">
      <span className="font-medium text-fg-muted">{label}</span>
      <span
        className={
          "inline-block h-1.5 w-1.5 rounded-full shrink-0 " +
          (isLocal ? "bg-status-done" : "bg-brand")
        }
        aria-hidden
      />
      <span>
        {t(isLocal ? "overlay.engine.local" : "overlay.engine.cloud")}
      </span>
      {detail ? (
        <span className="font-mono overflow-hidden text-ellipsis">
          {detail}
        </span>
      ) : null}
    </span>
  );
}

/**
 * When the user clicks the error text, we bring the main window to
 * the front and signal "show logs" via an event. App.tsx listens to
 * `app://focus-logs` and switches to the Logs tab.
 */
async function openLogsInMainWindow(): Promise<void> {
  try {
    const main = await WebviewWindow.getByLabel("main");
    if (main) {
      await main.show();
      await main.setFocus();
    }
    await emit("app://focus-logs");
  } catch {
    // The window API can fail on restrictive capabilities — in that
    // case the user still sees the full error text in the overlay
    // (wrap + title tooltip), which is tolerable.
  }
}

/**
 * If the text is longer than `max`, show only the end with an
 * ellipsis at the beginning — feels live ("…what is being said
 * right now"), fits the fixed-width overlay box without wrapping.
 */
function truncateStart(text: string, max: number): string {
  if (!text) return "";
  if (text.length <= max) return text;
  return `…${text.slice(text.length - max)}`;
}

/**
 * Honest recording-status LED. Pulses on a 1.2s scale/opacity cycle
 * — clearly identifiable as an LED indicator and (unlike the
 * previously used 3-bar equalizer) does not suggest a response to
 * the audio level. The actual level is not available in the
 * renderer; an IPC channel for it would be its own feature.
 */
function RecordingDot(): JSX.Element {
  return (
    <span
      className="shrink-0 inline-block h-3 w-3 rounded-full bg-status-recording animate-[vtx-rec-pulse_1200ms_ease-in-out_infinite] mr-1"
      aria-hidden
    />
  );
}

interface PhaseMeta {
  label: string;
  icon: JSX.Element;
  iconBg: string;
  iconColor: string;
}

function phaseMeta(
  t: TranslateFn,
  phase: Phase,
  errorMsg: string | null,
): PhaseMeta {
  switch (phase) {
    case "recording":
      return {
        label: t("overlay.phase.recording"),
        icon: <MicIcon />,
        iconBg: "bg-status-recording/15",
        iconColor: "text-status-recording",
      };
    case "transcribing":
      return {
        label: t("overlay.phase.transcribing"),
        icon: <WaveIcon />,
        iconBg: "bg-brand/15",
        iconColor: "text-brand",
      };
    case "postprocessing":
      return {
        label: t("overlay.phase.postprocessing"),
        icon: <SparkleIcon />,
        iconBg: "bg-status-processing/15",
        iconColor: "text-status-processing",
      };
    case "injecting":
      return {
        label: t("overlay.phase.injecting"),
        icon: <ArrowRightIcon />,
        iconBg: "bg-brand/15",
        iconColor: "text-brand",
      };
    case "error":
      return {
        label: errorMsg
          ? t("overlay.phase.error_with_msg", { message: errorMsg })
          : t("overlay.phase.error_generic"),
        icon: <AlertIcon />,
        iconBg: "bg-status-error/15",
        iconColor: "text-status-error",
      };
    case "idle":
    default:
      return {
        label: t("overlay.phase.recording"),
        icon: <MicIcon />,
        iconBg: "bg-status-recording/15",
        iconColor: "text-status-recording",
      };
  }
}

function MicIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <rect x="9" y="3" width="6" height="12" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 19v3" />
    </svg>
  );
}

function WaveIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M4 12h2l2-6 4 12 4-9 2 3h2" />
    </svg>
  );
}

function SparkleIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M5.6 18.4l2.8-2.8M15.6 8.4l2.8-2.8" />
    </svg>
  );
}

function ArrowRightIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M5 12h14M13 5l7 7-7 7" />
    </svg>
  );
}

function AlertIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-4 w-4"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M12 8v5M12 16h.01" />
    </svg>
  );
}
