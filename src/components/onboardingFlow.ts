// SPDX-License-Identifier: GPL-3.0-or-later
import type { Mode } from "../lib/types";

export type WizardStep = 1 | 2 | 3 | 4 | 5;
export type SttChoice = "local" | "chatgpt";

/** The two bundled modes whose descriptions promise local processing. */
export type DefaultModeKind = "exact" | "correction" | "other";

/**
 * Ids of the bundled default modes (`modes/defaults/<locale>/*.toml`) —
 * the German set names two of them differently.
 */
const DEFAULT_MODE_KINDS: Record<string, DefaultModeKind> = {
  exact: "exact",
  exakt: "exact",
  correction: "correction",
  korrektur: "correction",
  email: "other",
  chat: "other",
  issue: "other",
  agent: "other",
  improve: "other",
  reply: "other",
  transform: "other",
};

/**
 * Steps shown for a choice. With ChatGPT doing the post-processing too,
 * the xAI-key step (3) and the local-LLM step (4) are pointless.
 */
export function wizardSteps(
  choice: SttChoice,
  llmViaChatGpt: boolean,
): WizardStep[] {
  return choice === "chatgpt" && llmViaChatGpt ? [1, 2, 5] : [1, 2, 3, 4, 5];
}

/** ChatGPT only counts once the sign-in succeeded; otherwise setup continues with local Whisper. */
export function effectiveSttChoice(
  selected: SttChoice,
  connected: boolean,
): SttChoice {
  return selected === "chatgpt" && connected ? "chatgpt" : "local";
}

/**
 * Rewrites a bundled default mode to use ChatGPT. Only values still at
 * their bundled defaults change — local or xAI speech-to-text, and with
 * `llm` also xAI or local post-processing — so a mode the user already
 * customised keeps its providers. `describe` may replace the description
 * of modes whose bundled text promises local processing. Returns `null`
 * when nothing changes.
 */
export function applyChatGptChoice(
  mode: Mode,
  opts: {
    llm: boolean;
    describe: (kind: DefaultModeKind, next: Mode) => string | null;
  },
): Mode | null {
  const kind = Object.hasOwn(DEFAULT_MODE_KINDS, mode.id)
    ? DEFAULT_MODE_KINDS[mode.id]
    : undefined;
  if (!kind) return null;
  const next: Mode = { ...mode };
  let changed = false;

  if (mode.transcription === "local") {
    Object.assign(next, {
      transcription: "cloud",
      cloud_stt_provider: "chatgpt",
      whisper_model_slot: null,
      initial_prompt: null,
      whisper_beam_size: null,
    });
    changed = true;
  } else if (mode.cloud_stt_provider === "xai") {
    next.cloud_stt_provider = "chatgpt";
    changed = true;
  }

  if (opts.llm) {
    if (mode.processing === "cloud" && mode.cloud_llm_provider === "xai") {
      Object.assign(next, {
        cloud_llm_provider: "chatgpt",
        cloud_llm_model: null,
      });
      changed = true;
    } else if (mode.processing === "local") {
      Object.assign(next, {
        processing: "cloud",
        cloud_llm_provider: "chatgpt",
        cloud_llm_model: null,
        local_engine: null,
        ollama_model_tag: null,
        embedded_llm_slot: null,
        local_llm_model: null,
      });
      changed = true;
    }
  }

  if (!changed) return null;
  const description = opts.describe(kind, next);
  if (description) next.description = description;
  return next;
}
