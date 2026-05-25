// SPDX-License-Identifier: GPL-3.0-or-later
//
// Hardware-based recommendations for model slots. Shared between
// Settings.tsx (hardware status panel) and OnboardingWizard.tsx
// (step 4: LLM download).
//
// Keep the thresholds conservative: model footprint + inference
// working set should stay below ~50 % of total RAM so Whisper +
// browser + IDE aren't starved.

export interface LlmSlotRecommendation {
  /** Settings slug that lands in `Settings.llm_default_slot`. */
  slot: string;
  /** Display text for the UI banner. */
  label: string;
}

/**
 * Recommendation: which GGUF LLM slot fits the hardware.
 *
 * Thresholds (refresh May 2026 with Gemma 4 as the default for 8+ GB):
 * - 0 GB (detection not implemented on Windows) → light default.
 * - < 8 GB RAM → Gemma 3 1B (light, ~851 MB disk, the only option for
 *   constrained hardware — Gemma 4 is ~3 GB disk even in the
 *   smallest E2B format).
 * - 8–12 GB → **Gemma 4 E2B** (mid, ~3.1 GB disk, ~5 GB RAM 4-bit).
 *   Replaces Qwen 2.5 1.5B — better DE quality at similar latency.
 * - 12+ GB → **Gemma 4 E4B** (pro, ~5.1 GB disk, ~6 GB RAM 4-bit).
 *   Replaces Gemma 3 4B as the pro default — Apache 2.0, 256k
 *   context, 140+ languages, multimodal-capable (we only use text).
 */
export function recommendLlmSlot(totalRamGb: number): LlmSlotRecommendation {
  if (totalRamGb <= 0 || totalRamGb < 8) {
    return { slot: "gemma3-1b-it-q5_k_m", label: "Gemma 3 1B (Light)" };
  }
  if (totalRamGb < 12) {
    return {
      slot: "gemma4-e2b-it-q5_k_m",
      label: "Gemma 4 E2B (Mid)",
    };
  }
  return { slot: "gemma4-e4b-it-q5_k_m", label: "Gemma 4 E4B (Pro)" };
}
