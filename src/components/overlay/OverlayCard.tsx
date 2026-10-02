// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode, JSX } from "react";
import { useLocale, useT, type TranslateFn } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import {
  engineSteps,
  formatElapsed,
  tailWords,
  type EngineSegment,
  type EngineStatus,
  type ErrorStage,
  type Phase,
  type StepState,
} from "../../lib/overlayModel";
import LevelWaveform, { type LevelSubscribe } from "./LevelWaveform";

export interface OverlayCardProps {
  phase: Phase;
  /** Bumped at every recording start — replays the entrance. */
  session: number;
  engine: EngineStatus | null;
  partial: string;
  /** Time in the current phase (recording: since the start). */
  elapsedMs: number;
  silent: boolean;
  /** Readable menu hotkey, `null` while unknown. */
  hotkey: string | null;
  usageHint: { text: string; level: "warn" | "reached" } | null;
  error: { message: string | null; stage: ErrorStage } | null;
  levels: LevelSubscribe;
  onErrorClick: () => void;
}

/** Characters of live transcript that fit the middle row. */
const PARTIAL_CHARS = 70;

/**
 * The recording overlay card ("Editorial Card"): phase + mode, timer and
 * waveform on top; live transcript or a hint in the middle; the engine
 * line as a progress stepper below. Pure view — the container
 * (src/views/Overlay.tsx) and the dev preview feed it.
 */
export default function OverlayCard(props: OverlayCardProps): JSX.Element {
  const { phase, engine, error } = props;
  const t = useT();
  const locale = useLocale();
  const isError = phase === "error";

  if (phase === "idle") {
    // Blank while hidden: the next show must not flash the last frame.
    return <div className="h-full w-full" />;
  }

  const tone = TONES[phase];
  const label = isError
    ? t(`overlay.error.title.${error?.stage ?? "generic"}`)
    : t(`overlay.phase.${phase}`);
  const seconds = Math.floor(props.elapsedMs / 1000);
  const clock =
    phase === "recording"
      ? formatElapsed(props.elapsedMs)
      : (phase === "transcribing" || phase === "postprocessing") && seconds > 0
        ? formatNumber(seconds, locale, {
            style: "unit",
            unit: "second",
            unitDisplay: "narrow",
          })
        : null;
  const wave =
    phase === "recording"
      ? "live"
      : phase === "transcribing" || phase === "postprocessing"
        ? phase
        : null;

  return (
    <div
      key={props.session}
      onClick={isError ? props.onErrorClick : undefined}
      className={
        "vtx-enter h-full w-full rounded-lg vtx-glass px-4 py-2 flex flex-col justify-center gap-1 transition-colors duration-200 " +
        (isError
          ? "pointer-events-auto cursor-pointer border-status-error/40!"
          : "")
      }
    >
      <div className="flex items-center gap-3">
        <span
          key={phase}
          className={`shrink-0 inline-flex items-center justify-center h-7 w-7 rounded-md transition-colors duration-200 ${tone.bg} ${tone.fg} ${isError ? "vtx-pop" : ""}`}
          aria-hidden
        >
          {tone.icon}
        </span>
        <p
          key={`${phase}-label`}
          className="vtx-phase-in flex-1 min-w-0 truncate text-sm leading-snug font-medium text-fg"
        >
          {label}
          {engine?.mode_name && !isError ? (
            <span className="font-normal text-fg-muted">
              {" "}
              · {engine.mode_name}
            </span>
          ) : null}
        </p>
        {clock ? (
          <span className="shrink-0 text-xs tabular-nums text-fg-muted">
            {clock}
          </span>
        ) : null}
        {wave ? <LevelWaveform mode={wave} subscribe={props.levels} /> : null}
      </div>

      <MiddleRow {...props} t={t} />

      {isError ? (
        <p className="pl-10 text-xxs text-fg-faint truncate">
          {props.hotkey
            ? t("overlay.error.action", { hotkey: props.hotkey })
            : t("overlay.error.action_generic")}
        </p>
      ) : engine ? (
        <EngineLine
          engine={engine}
          phase={phase}
          usageHint={props.usageHint}
          t={t}
        />
      ) : null}
    </div>
  );
}

function MiddleRow({
  phase,
  partial,
  silent,
  hotkey,
  error,
  t,
}: OverlayCardProps & { t: TranslateFn }): JSX.Element | null {
  const row = "pl-10 pr-1 text-xs leading-snug truncate";
  if (phase === "error") {
    return error?.message ? (
      <p className={`${row} text-fg-muted`} title={error.message}>
        {error.message}
      </p>
    ) : null;
  }
  if (phase !== "recording") return null;
  if (partial) return <LiveWords text={partial} className={`${row} text-fg`} />;
  if (silent) {
    return (
      <p className={`${row} text-status-processing`}>
        {t("overlay.hint.silent")}
      </p>
    );
  }
  return hotkey ? (
    <p className={`${row} text-fg-faint`}>
      {t("overlay.hint.stop", { hotkey })}
    </p>
  ) : null;
}

/**
 * The newest transcript words. Keys are absolute word positions, so only
 * words that are new (or revised) mount — and fade in.
 */
function LiveWords({
  text,
  className,
}: {
  text: string;
  className: string;
}): JSX.Element {
  const { words, truncated } = tailWords(text, PARTIAL_CHARS);
  return (
    <p className={className} title={text}>
      {truncated ? <span className="text-fg-faint">… </span> : null}
      {words.map(({ index, word }) => (
        <span key={`${index}:${word}`} className="vtx-word-in inline-block">
          {word}&nbsp;
        </span>
      ))}
    </p>
  );
}

function EngineLine({
  engine,
  phase,
  usageHint,
  t,
}: {
  engine: EngineStatus;
  phase: Phase;
  usageHint: OverlayCardProps["usageHint"];
  t: TranslateFn;
}): JSX.Element {
  const steps = engineSteps(phase);
  return (
    <div className="flex items-center gap-1.5 text-[10px] text-fg-faint pl-10 pr-1 overflow-hidden whitespace-nowrap">
      <EngineSeg
        label={t("overlay.engine.stt")}
        seg={engine.stt}
        state={steps.stt}
        t={t}
      />
      {engine.llm ? (
        <>
          <span className="text-outline" aria-hidden>
            ·
          </span>
          <EngineSeg
            label={t("overlay.engine.llm")}
            seg={engine.llm}
            state={steps.llm}
            t={t}
          />
        </>
      ) : null}
      {usageHint ? (
        <span
          className={
            "ml-auto shrink-0 " +
            (usageHint.level === "reached"
              ? "text-status-error"
              : "text-status-processing")
          }
        >
          {usageHint.text}
        </span>
      ) : null}
    </div>
  );
}

/** One engine segment: label, local/cloud dot, location and
 *  `provider·model`; the running stage is underlined, finished ones get ✓. */
function EngineSeg({
  label,
  seg,
  state,
  t,
}: {
  label: string;
  seg: EngineSegment;
  state: StepState;
  t: TranslateFn;
}): JSX.Element {
  const isLocal = seg.location === "local";
  const detail = seg.provider
    ? seg.model
      ? `${seg.provider}·${seg.model}`
      : seg.provider
    : seg.model;
  return (
    <span
      className={
        "inline-flex items-center gap-1 overflow-hidden border-b-2 transition-colors duration-200 " +
        (state === "active"
          ? "border-brand text-fg"
          : state === "done"
            ? "border-transparent text-fg-muted"
            : "border-transparent")
      }
    >
      <span className="font-medium text-fg-muted">{label}</span>
      {state === "done" ? (
        <span className="text-status-done" aria-hidden>
          ✓
        </span>
      ) : (
        <span
          className={
            "inline-block h-1.5 w-1.5 rounded-full shrink-0 " +
            (isLocal ? "bg-status-done" : "bg-brand")
          }
          aria-hidden
        />
      )}
      <span>
        {t(isLocal ? "overlay.engine.local" : "overlay.engine.cloud")}
      </span>
      {detail ? (
        <span
          className="font-mono overflow-hidden text-ellipsis min-w-0"
          title={detail}
        >
          {detail}
        </span>
      ) : null}
    </span>
  );
}

const TONES: Record<
  Exclude<Phase, "idle">,
  { bg: string; fg: string; icon: JSX.Element }
> = {
  recording: {
    bg: "bg-status-recording/15",
    fg: "text-status-recording",
    icon: <MicIcon />,
  },
  transcribing: { bg: "bg-brand/15", fg: "text-brand", icon: <WaveIcon /> },
  postprocessing: {
    bg: "bg-status-processing/15",
    fg: "text-status-processing",
    icon: <SparkleIcon />,
  },
  injecting: { bg: "bg-brand/15", fg: "text-brand", icon: <ArrowRightIcon /> },
  error: {
    bg: "bg-status-error/15",
    fg: "text-status-error",
    icon: <AlertIcon />,
  },
};

function Icon({ children }: { children: ReactNode }): JSX.Element {
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
      {children}
    </svg>
  );
}

function MicIcon(): JSX.Element {
  return (
    <Icon>
      <rect x="9" y="3" width="6" height="12" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 19v3" />
    </Icon>
  );
}

function WaveIcon(): JSX.Element {
  return (
    <Icon>
      <path d="M4 12h2l2-6 4 12 4-9 2 3h2" />
    </Icon>
  );
}

function SparkleIcon(): JSX.Element {
  return (
    <Icon>
      <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.6 5.6l2.8 2.8M15.6 15.6l2.8 2.8M5.6 18.4l2.8-2.8M15.6 8.4l2.8-2.8" />
    </Icon>
  );
}

function ArrowRightIcon(): JSX.Element {
  return (
    <Icon>
      <path d="M5 12h14M13 5l7 7-7 7" />
    </Icon>
  );
}

function AlertIcon(): JSX.Element {
  return (
    <Icon>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 8v5M12 16h.01" />
    </Icon>
  );
}
