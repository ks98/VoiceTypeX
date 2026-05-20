// SPDX-License-Identifier: GPL-3.0-or-later
//
// Hardware-basierte Empfehlungen fuer Modell-Slots. Geteilt zwischen
// Settings.tsx (Hardware-Status-Panel) und OnboardingWizard.tsx
// (Step 4: LLM-Download).
//
// Halte die Schwellen konservativ: Modell-Footprint + Inferenz-Working-
// Set sollten unter ~50 % des Gesamt-RAM bleiben, damit Whisper +
// Browser + IDE nicht ausgehungert werden.

export interface LlmSlotRecommendation {
  /** Settings-Slug, der in `Settings.llm_default_slot` landet. */
  slot: string;
  /** Anzeigetext fuer UI-Banner. */
  label: string;
}

/**
 * Empfehlung welcher GGUF-LLM-Slot zur Hardware passt.
 *
 * Schwellen:
 * - 0 GB (Detection nicht implementiert auf Windows) → Light-Default.
 * - < 8 GB RAM → Gemma 3 1B (Light, ~851 MB).
 * - 8–12 GB → Qwen 2.5 1.5B (Mittel, ~1.3 GB, gut auf Code).
 * - 12+ GB → Gemma 3 4B (Pro, ~2.8 GB, beste DE-Qualitaet).
 */
export function recommendLlmSlot(totalRamGb: number): LlmSlotRecommendation {
  if (totalRamGb <= 0 || totalRamGb < 8) {
    return { slot: "gemma3-1b-it-q5_k_m", label: "Gemma 3 1B (Light)" };
  }
  if (totalRamGb < 12) {
    return {
      slot: "qwen2.5-1.5b-instruct-q5_k_m",
      label: "Qwen 2.5 1.5B (Mittel)",
    };
  }
  return { slot: "gemma3-4b-it-q5_k_m", label: "Gemma 3 4B (Pro)" };
}
