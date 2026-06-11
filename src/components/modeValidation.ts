// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pure save-gating validation for the ModeEditor. Kept out of the .tsx
// component so it has no React/`t` dependency and is unit-testable in
// isolation (see ModeEditor.test.ts).
import type { Mode } from "../lib/types";

function inRange(value: number | null, min: number, max: number): boolean {
  if (value === null) return true;
  if (!Number.isFinite(value)) return false;
  return value >= min && value <= max;
}

/**
 * Stable reason codes for the save-gating validation. Each maps 1:1 to a
 * `mode_editor.reason.<code>` i18n key; the component renders them via
 * `t()`. Returned as codes (not translated strings) so the gating logic is
 * a pure function, unit-testable without React or a `t` stub.
 */
export type BlockingReason =
  | "id_missing"
  | "id_invalid"
  | "name_missing"
  | "cloud_stt_provider"
  | "cloud_llm_provider"
  | "ollama_tag"
  | "selection_needs_llm"
  | "system_prompt"
  | "sampling";

/**
 * Pure save-gating validation. Returns the blocking reasons in display
 * order; an empty array means the draft can be saved. `windows` mirrors
 * `isWindows()` — passed in (not read here) to keep the function pure and
 * independent of the Tauri global.
 */
export function computeBlockingReasons(
  draft: Mode,
  windows: boolean,
): BlockingReason[] {
  const idValid = /^[a-zA-Z0-9_-]+$/.test(draft.id);
  const isCloudSTT = draft.transcription === "cloud";
  const isCloudLLM = draft.processing === "cloud";
  const isLocalLLM = draft.processing === "local";
  const needsSystemPrompt = draft.processing !== "none";
  const isSelectionInput = draft.input === "selection";
  // Mirrors the component's `localEngine` derivation: embedded is the
  // default off-Windows, but Windows has no embedded engine (issue #1) so
  // a local-LLM mode there is always Ollama-backed.
  const localEngine: "embedded" | "ollama" = windows
    ? "ollama"
    : draft.local_engine === "ollama"
      ? "ollama"
      : "embedded";
  const needsOllamaTag = isLocalLLM && localEngine === "ollama";

  const samplingValid =
    inRange(draft.temperature, 0, 2) &&
    inRange(draft.top_p, 0, 1) &&
    inRange(draft.repeat_penalty, 0.5, 2) &&
    inRange(draft.max_tokens, 1, 8192);

  const reasons: BlockingReason[] = [];
  if (draft.id.length === 0) reasons.push("id_missing");
  else if (!idValid) reasons.push("id_invalid");
  if (draft.name.length === 0) reasons.push("name_missing");
  if (isCloudSTT && !draft.cloud_stt_provider)
    reasons.push("cloud_stt_provider");
  if (isCloudLLM && !draft.cloud_llm_provider)
    reasons.push("cloud_llm_provider");
  if (needsOllamaTag && !draft.ollama_model_tag) reasons.push("ollama_tag");
  if (isSelectionInput && draft.processing === "none")
    reasons.push("selection_needs_llm");
  if (
    needsSystemPrompt &&
    (draft.system_prompt === null || draft.system_prompt.length === 0)
  )
    reasons.push("system_prompt");
  if (!samplingValid) reasons.push("sampling");
  return reasons;
}
