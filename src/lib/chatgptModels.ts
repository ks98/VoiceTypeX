// SPDX-License-Identifier: GPL-3.0-or-later

// Models offered for the experimental `chatgpt` LLM provider. A curated
// list, because the backend's model catalog returns nothing for our honest
// client_version (we never pose as the Codex CLI). Each entry answered the
// /codex/responses call with a ChatGPT Plus account on 2026-09-28 — see
// docs/PROVIDERS.md. Brand names, deliberately not translated.
export const CHATGPT_MODELS: readonly { slug: string; label: string }[] = [
  { slug: "gpt-6-luna", label: "GPT-6 Luna" },
  { slug: "gpt-6-sol", label: "GPT-6 Sol" },
  { slug: "gpt-6-astra", label: "GPT-6 Astra" },
];

/** Used by the backend when a mode leaves the model empty. Keep in sync
 * with `DEFAULT_MODEL` in src-tauri/src/processing/cloud/chatgpt.rs. */
export const CHATGPT_DEFAULT_MODEL = "gpt-6-luna";

export function isListedChatGptModel(model: string | null): boolean {
  return CHATGPT_MODELS.some((m) => m.slug === model);
}
