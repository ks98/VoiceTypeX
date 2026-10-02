// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pure view logic of the recording overlay (src/views/Overlay.tsx and
// src/components/overlay/*), kept free of React and Tauri so it is unit
// tested and the dev preview can render every state in a plain browser.

export type Phase =
  | "idle"
  | "recording"
  | "transcribing"
  | "postprocessing"
  | "injecting"
  | "error";

export type EngineSegment = {
  location: "local" | "cloud";
  provider: string | null;
  model: string;
};

/** Payload of `app://active-engine` (Rust `core::modes::EngineStatus`). */
export type EngineStatus = {
  mode_name: string;
  stt: EngineSegment;
  llm: EngineSegment | null;
};

export type StepState = "pending" | "active" | "done";

/** The engine line doubles as a progress stepper: which stage runs now. */
export function engineSteps(phase: Phase): { stt: StepState; llm: StepState } {
  switch (phase) {
    case "transcribing":
      return { stt: "active", llm: "pending" };
    case "postprocessing":
      return { stt: "done", llm: "active" };
    case "injecting":
      return { stt: "done", llm: "done" };
    default:
      return { stt: "pending", llm: "pending" };
  }
}

export type ErrorStage =
  "recording" | "transcribing" | "postprocessing" | "injecting" | "generic";

/** The stage that failed = the last phase before `error`. */
export function errorStage(previous: Phase | null): ErrorStage {
  switch (previous) {
    case "recording":
    case "transcribing":
    case "postprocessing":
    case "injecting":
      return previous;
    default:
      return "generic";
  }
}

/** Recording clock: `0:07`, `1:05`, `12:30`. */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

/** Nothing louder than this since the start counts as "no audio". */
export const SPEECH_LEVEL = 0.15;
export const SILENCE_HINT_MS = 4000;

/**
 * "Is the microphone muted?" — once per recording: only while nothing
 * speech-like has been heard since the start.
 */
export function silenceHintDue(elapsedMs: number, maxLevel: number): boolean {
  return elapsedMs >= SILENCE_HINT_MS && maxLevel < SPEECH_LEVEL;
}

const HOTKEY_TOKENS: Record<string, string> = {
  commandorcontrol: "Ctrl",
  cmdorctrl: "Ctrl",
  control: "Ctrl",
  ctrl: "Ctrl",
  alt: "Alt",
  option: "Alt",
  shift: "Shift",
  super: "Super",
  meta: "Super",
  command: "Super",
  cmd: "Super",
};

/**
 * Human-readable hotkey from a Tauri accelerator
 * (`CommandOrControl+Alt+Space` → `Ctrl+Alt+Space`). Wayland portal
 * descriptions are already readable and pass through unchanged.
 */
export function hotkeyLabel(accelerator: string): string {
  return accelerator
    .split("+")
    .map((token) => token.trim())
    .filter(Boolean)
    .map(
      (token) =>
        HOTKEY_TOKENS[token.toLowerCase()] ??
        (token.length === 1
          ? token.toUpperCase()
          : token[0]!.toUpperCase() + token.slice(1)),
    )
    .join("+");
}

export interface IndexedWord {
  /** Position in the whole transcript — the stable React key. */
  index: number;
  word: string;
}

/**
 * The newest words of the live transcript that fit `maxChars`. Words keep
 * their absolute index, so a word that stays on screen keeps its key and
 * only newly arrived (or revised) words animate in.
 */
export function tailWords(
  text: string,
  maxChars: number,
): { words: IndexedWord[]; truncated: boolean } {
  const all = text.split(/\s+/).filter(Boolean);
  const words: IndexedWord[] = [];
  let used = 0;
  for (let i = all.length - 1; i >= 0; i--) {
    const word = all[i]!;
    const cost = word.length + (words.length > 0 ? 1 : 0);
    if (words.length > 0 && used + cost > maxChars) break;
    used += cost;
    words.unshift({ index: i, word });
  }
  return { words, truncated: words.length < all.length };
}
