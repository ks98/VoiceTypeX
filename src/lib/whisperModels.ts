// SPDX-License-Identifier: GPL-3.0-or-later
//
// Single source of truth for how the local Whisper model slots are
// presented in the UI (Settings + ModeEditor model cards). Keeps the
// slot slug, its i18n name/tagline keys, and the qualitative
// speed/accuracy/footprint axes in one place so Settings and ModeEditor
// cannot drift apart.
//
// The `speed` and `accuracy` scores are a 1..5 QUALITATIVE ranking
// derived from the 2026-06 model research (architecture: turbo = 4
// decoder layers vs large-v3's 32; quantization Q5/Q8/F16; small = 244M
// params). They are NOT measured RTF/WER numbers — the on-device
// measurement would come from the Soundcheck harness (issue #9). They
// exist only to let a user rank "weaker vs stronger" at a glance.
//
// `german` marks the DE fine-tune slots: their `accuracy` score is the
// GENERIC (cross-language) accuracy like the others, but for German
// specifically they are the best — surfaced via the DE badge + tagline +
// the language-aware recommender, not via the neutral accuracy axis.

import type { HardwareReport } from "./tauri";

export interface WhisperModelMeta {
  /** Settings slug persisted in `whisper_default_slot` / `whisper_model_slot`. */
  slot: string;
  /** i18n key for the short display name (e.g. "Deutsch-Pro Q8"). */
  nameKey: string;
  /** i18n key for the one-line tagline. */
  taglineKey: string;
  /** Qualitative tempo, 1 (slowest) .. 5 (fastest). */
  speed: 1 | 2 | 3 | 4 | 5;
  /** Qualitative generic accuracy, 1 (weakest) .. 5 (best). */
  accuracy: 1 | 2 | 3 | 4 | 5;
  /** Approx download size in MB (matches model_downloader.rs). */
  sizeMb: number;
  /** Realistic minimum total RAM in GB to run without swapping. */
  minRamGb: number;
  /** True for the German fine-tune slots (gets the DE badge). */
  german: boolean;
}

// Order = display order: German fine-tunes first (DE-first product),
// then generic, then the frugal fallback last.
export const WHISPER_MODELS: WhisperModelMeta[] = [
  {
    slot: "large-v3-turbo-german-q8_0",
    nameKey: "whisper_models.de_pro_q8.name",
    taglineKey: "whisper_models.de_pro_q8.tagline",
    speed: 4,
    accuracy: 4,
    sizeMb: 874,
    minRamGb: 8,
    german: true,
  },
  {
    slot: "large-v3-turbo-german-q5_0",
    nameKey: "whisper_models.de_pro_q5.name",
    taglineKey: "whisper_models.de_pro_q5.tagline",
    speed: 4,
    accuracy: 4,
    sizeMb: 574,
    minRamGb: 8,
    german: true,
  },
  {
    slot: "large-v3-turbo-q8_0",
    nameKey: "whisper_models.universal_q8.name",
    taglineKey: "whisper_models.universal_q8.tagline",
    speed: 4,
    accuracy: 4,
    sizeMb: 874,
    minRamGb: 8,
    german: false,
  },
  {
    slot: "large-v3-turbo-q5_0",
    nameKey: "whisper_models.universal_q5.name",
    taglineKey: "whisper_models.universal_q5.tagline",
    speed: 4,
    accuracy: 3,
    sizeMb: 547,
    minRamGb: 6,
    german: false,
  },
  {
    slot: "large-v3-turbo",
    nameKey: "whisper_models.universal_f16.name",
    taglineKey: "whisper_models.universal_f16.tagline",
    speed: 3,
    accuracy: 5,
    sizeMb: 1624,
    minRamGb: 8,
    german: false,
  },
  {
    slot: "small-q5_1",
    nameKey: "whisper_models.small.name",
    taglineKey: "whisper_models.small.tagline",
    speed: 5,
    accuracy: 2,
    sizeMb: 190,
    minRamGb: 4,
    german: false,
  },
];

export function whisperModelBySlot(slot: string): WhisperModelMeta | undefined {
  return WHISPER_MODELS.find((m) => m.slot === slot);
}

/**
 * Hardware- and language-aware recommendation for the default Whisper
 * slot. Mirrors `recommendLlmSlot`, but factors in the DE-first product:
 * when the UI language is German, the fine-tune slots are preferred.
 *
 * Thresholds (from the 2026-06 research):
 * - RAM detection unavailable (0, e.g. Windows) → assume a modern machine
 *   (8+ GB) and recommend the Q8 sweet-spot.
 * - < 5 GB → `small-q5_1` is the only slot that fits.
 * - 5–8 GB → the lighter Q5 quant.
 * - ≥ 8 GB → Q8 (near-F16 quality, Vulkan-safe vs the Q5 iGPU
 *   degradation, whisper.cpp #3047).
 */
export function recommendWhisperSlot(
  hardware: HardwareReport | null,
  preferGerman: boolean,
): string {
  const ram = hardware?.total_ram_gb ?? 0;

  if (ram > 0 && ram < 5) return "small-q5_1";
  if (ram > 0 && ram < 8) {
    return preferGerman ? "large-v3-turbo-german-q5_0" : "large-v3-turbo-q5_0";
  }
  return preferGerman ? "large-v3-turbo-german-q8_0" : "large-v3-turbo-q8_0";
}
