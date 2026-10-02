// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useRef, type JSX } from "react";

export type LevelListener = (level: number) => void;
/** Registers a listener for 0..1 input levels; returns the unsubscribe. */
export type LevelSubscribe = (listener: LevelListener) => () => void;

export type WaveformMode = "live" | "transcribing" | "postprocessing";

const BARS = 28;
/** The newest bars take the recording colour — "this is you, now". */
const FRESH_BARS = 6;
const BASELINE = 0.12;

/**
 * 28 thin bars. `live`: a scrolling history of the microphone level, newest
 * on the right. Otherwise a travelling wave that shows work in progress.
 *
 * Live updates arrive at 25 Hz and write `transform` straight to the DOM —
 * no React render per level event.
 */
export default function LevelWaveform({
  mode,
  subscribe,
}: {
  mode: WaveformMode;
  subscribe: LevelSubscribe;
}): JSX.Element {
  const bars = useRef<(HTMLSpanElement | null)[]>([]);

  useEffect(() => {
    if (mode !== "live") return;
    const history = new Array<number>(BARS).fill(0);
    return subscribe((level) => {
      history.shift();
      history.push(level);
      history.forEach((v, i) => {
        const bar = bars.current[i];
        if (bar)
          bar.style.transform = `scaleY(${BASELINE + (1 - BASELINE) * v})`;
      });
    });
  }, [mode, subscribe]);

  const live = mode === "live";
  const color = live
    ? null
    : mode === "transcribing"
      ? "bg-brand/70"
      : "bg-status-processing/80";

  return (
    // Keyed by mode: a fresh set of bars drops the live transforms.
    <span
      key={mode}
      className="flex h-[22px] shrink-0 items-center gap-[3px]"
      aria-hidden
    >
      {Array.from({ length: BARS }, (_, i) => (
        <span
          key={i}
          ref={(el) => {
            bars.current[i] = el;
          }}
          className={
            "h-full w-[2px] rounded-full " +
            (live
              ? "transition-transform duration-75 ease-linear " +
                (i >= BARS - FRESH_BARS ? "bg-status-recording" : "bg-fg/35")
              : `${color} ${mode === "transcribing" ? "vtx-bar-wave" : "vtx-bar-wave-slow"}`)
          }
          style={{
            transform: `scaleY(${BASELINE})`,
            // Negative: the wave is already travelling when it mounts.
            animationDelay: live ? undefined : `${(i - BARS) * 40}ms`,
          }}
        />
      ))}
    </span>
  );
}
