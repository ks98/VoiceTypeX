// SPDX-License-Identifier: GPL-3.0-or-later
//
// Single source of truth for the embedded-LLM slot list, consumed by the
// global default picker (Settings) and the per-mode override picker
// (ModeEditor). The slugs mirror the backend mapping
// (`LlmModelSlot::from_setting`); a new/removed slot is edited here once.
//
// Labels come via i18n key rather than hardcoded strings, so a locale
// switch takes effect. The two call sites use distinct key namespaces
// (`settings.llm.slot.*` is verbose with RAM hints, `mode_editor.llm_slot.*`
// is terser), so each prepends its own prefix to the shared `keySuffix`.
// (The Whisper slots live in src/lib/whisperModels.ts.)

export interface LlmSlotMeta {
  /** Settings slug persisted in `llm_default_slot` / `embedded_llm_slot`. */
  slot: string;
  /** Trailing i18n key segment, shared across both namespaces. */
  keySuffix: string;
}

// Order = display order.
export const LLM_SLOTS: LlmSlotMeta[] = [
  { slot: "gemma4-e4b-it-q5_k_m", keySuffix: "gemma4_e4b" },
  { slot: "gemma4-e2b-it-q5_k_m", keySuffix: "gemma4_e2b" },
  { slot: "gemma3-1b-it-q5_k_m", keySuffix: "gemma3_1b" },
  { slot: "gemma3-4b-it-q5_k_m", keySuffix: "gemma3_4b" },
  { slot: "llama3.2-1b-instruct-q5_k_m", keySuffix: "llama32_1b" },
  { slot: "qwen2.5-1.5b-instruct-q5_k_m", keySuffix: "qwen25_15b" },
];
