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
 * Schwellen (Refresh Mai 2026 mit Gemma 4 als Default fuer 8+ GB):
 * - 0 GB (Detection nicht implementiert auf Windows) → Light-Default.
 * - < 8 GB RAM → Gemma 3 1B (Light, ~851 MB Disk, einzige Option fuer
 *   knappe Hardware — Gemma 4 ist im kleinsten E2B-Format ~3 GB Disk).
 * - 8–12 GB → **Gemma 4 E2B** (Mittel, ~3,1 GB Disk, ~5 GB RAM 4-bit).
 *   Loest Qwen 2.5 1.5B ab — bessere DE-Qualitaet bei aehnlicher Latenz.
 * - 12+ GB → **Gemma 4 E4B** (Pro, ~5,1 GB Disk, ~6 GB RAM 4-bit).
 *   Loest Gemma 3 4B als Pro-Default ab — Apache 2.0, 256k Context,
 *   140+ Sprachen, multimodal-faehig (wir nutzen nur Text).
 */
export function recommendLlmSlot(totalRamGb: number): LlmSlotRecommendation {
  if (totalRamGb <= 0 || totalRamGb < 8) {
    return { slot: "gemma3-1b-it-q5_k_m", label: "Gemma 3 1B (Light)" };
  }
  if (totalRamGb < 12) {
    return {
      slot: "gemma4-e2b-it-q5_k_m",
      label: "Gemma 4 E2B (Mittel)",
    };
  }
  return { slot: "gemma4-e4b-it-q5_k_m", label: "Gemma 4 E4B (Pro)" };
}
