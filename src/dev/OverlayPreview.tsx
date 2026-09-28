// SPDX-License-Identifier: GPL-3.0-or-later
//
// Dev-only preview of every overlay state (`?window=overlay-preview`, only
// routed under `import.meta.env.DEV`, so it is not in release builds).
// Renders the pure OverlayCard with fixtures in a plain browser — no Tauri.
//
//   ?theme=dark   dark tokens        ?lang=de   locale
//   ?freeze=1     no entrance motion, paused waves, fixed levels (screenshots)
//
// Labels here are developer-facing and deliberately not translated.
import { useEffect } from "react";
import OverlayCard, {
  type OverlayCardProps,
} from "../components/overlay/OverlayCard";
import type { LevelSubscribe } from "../components/overlay/LevelWaveform";
import type { EngineStatus } from "../lib/overlayModel";
import { pickSupported, useI18nStore } from "../i18n";

const params = new URLSearchParams(window.location.search);
const FREEZE = params.get("freeze") === "1";

const LOCAL: EngineStatus = {
  mode_name: "Correction",
  stt: { location: "local", provider: null, model: "large-v3-turbo-q5" },
  llm: { location: "local", provider: null, model: "qwen3-4b" },
};
const CHATGPT: EngineStatus = {
  mode_name: "E-mail",
  stt: { location: "cloud", provider: "chatgpt", model: "" },
  llm: { location: "cloud", provider: "chatgpt", model: "gpt-6-luna" },
};
const NO_LLM: EngineStatus = { ...LOCAL, mode_name: "Exact", llm: null };

/** A speech-like level signal: syllable bursts with pauses. */
function speechLevel(tick: number): number {
  const syllable =
    Math.max(0, Math.sin(tick * 0.9)) * (0.55 + 0.45 * Math.sin(tick * 0.13));
  const pause = Math.sin(tick * 0.045) > 0.75 ? 0.1 : 1;
  return Math.min(1, syllable * pause + 0.04 * Math.random());
}

const levels: LevelSubscribe = (listener) => {
  if (FREEZE) {
    for (let i = 0; i < 28; i++) listener(Math.abs(Math.sin(i * 0.7)) * 0.9);
    return () => undefined;
  }
  let tick = 0;
  const id = window.setInterval(() => listener(speechLevel(tick++)), 40);
  return () => window.clearInterval(id);
};

const base: OverlayCardProps = {
  phase: "recording",
  session: 1,
  engine: LOCAL,
  partial: "",
  elapsedMs: 7_400,
  silent: false,
  hotkey: "Ctrl+Alt+Space",
  usageHint: null,
  error: null,
  levels,
  onErrorClick: () => undefined,
};

const FIXTURES: [string, Partial<OverlayCardProps>][] = [
  ["recording — stop hint", {}],
  [
    "recording — live transcript",
    {
      elapsedMs: 12_100,
      partial:
        "and then we ship the new overlay before Friday so that everybody can try it over the weekend",
    },
  ],
  ["recording — silence hint", { elapsedMs: 4_600, silent: true }],
  [
    "recording — ChatGPT, usage warning",
    { engine: CHATGPT, usageHint: { text: "5h 84 %", level: "warn" } },
  ],
  [
    "transcribing — ChatGPT",
    { phase: "transcribing", engine: CHATGPT, elapsedMs: 3_200 },
  ],
  ["post-processing — local", { phase: "postprocessing", elapsedMs: 1_300 }],
  ["inserting", { phase: "injecting", elapsedMs: 0 }],
  [
    "error — short",
    {
      phase: "error",
      error: { message: "No default input device", stage: "recording" },
    },
  ],
  [
    "error — long backend message",
    {
      phase: "error",
      error: {
        message:
          "ChatGPT usage limit reached for the 5-hour window — resets in ~2 h 14 min. Switch the mode to another provider or wait.",
        stage: "transcribing",
      },
    },
  ],
  [
    "no LLM — limit reached",
    {
      engine: NO_LLM,
      usageHint: { text: "Limit reached · Mon 09:12", level: "reached" },
    },
  ],
];

export default function OverlayPreview(): JSX.Element {
  useEffect(() => {
    document.documentElement.classList.toggle(
      "dark",
      params.get("theme") === "dark",
    );
    const lang = params.get("lang");
    if (lang) useI18nStore.setState({ locale: pickSupported(lang) });
  }, []);

  return (
    <div className="min-h-screen p-6 flex flex-wrap gap-6 bg-canvas">
      {FREEZE ? (
        <style>{`.vtx-enter,.vtx-phase-in,.vtx-word-in,.vtx-pop{animation:none!important}
          .vtx-bar-wave,.vtx-bar-wave-slow{animation-play-state:paused!important}`}</style>
      ) : null}
      {FIXTURES.map(([name, overrides]) => (
        <figure key={name} className="flex flex-col gap-1">
          <figcaption className="text-xs text-fg-muted font-mono">
            {name}
          </figcaption>
          {/* Same size and padding as the overlay window, on a fake desktop. */}
          <div
            className="h-[120px] w-[520px] p-2 rounded-md"
            style={{
              background:
                "linear-gradient(135deg, #5b7fa6 0%, #c9a27e 55%, #3e5566 100%)",
            }}
          >
            <OverlayCard {...base} {...overrides} />
          </div>
        </figure>
      ))}
    </div>
  );
}
