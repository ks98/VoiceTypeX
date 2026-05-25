// SPDX-License-Identifier: GPL-3.0-or-later
// TypeScript mirror of the Rust data types.
// Convention: snake_case fields, because the backend uses serde defaults.

export type TranscriptionTarget = "local" | "cloud";
export type ProcessingTarget = "none" | "local" | "cloud";
export type InjectionMethod = "clipboard" | "keystrokes";

export interface Mode {
  id: string;
  name: string;
  description: string;
  /** Legacy field; optional and unused since the menu-hotkey rework. */
  hotkey?: string | null;
  transcription: TranscriptionTarget;
  processing: ProcessingTarget;
  cloud_stt_provider: string | null;
  cloud_llm_provider: string | null;
  cloud_llm_model: string | null;
  /**
   * @deprecated since the phase-3b refactor — replaced by
   * `ollama_model_tag`. Remains in the type mirror only for migration
   * from old TOMLs; on save the backend mirrors the field back into
   * `ollama_model_tag`.
   */
  local_llm_model: string | null;
  /**
   * Engine for `processing = "local"`. `"embedded"` (default when
   * null, the built-in llama-cpp-2 path) or `"ollama"` (external
   * daemon, opt-in). Old TOMLs without an explicit field but with
   * `local_llm_model` are auto-migrated to `"ollama"` by the backend.
   */
  local_engine: string | null;
  /** Phase 3b: Ollama model tag (e.g. "llama3.2:3b") when
   * engine=ollama. */
  ollama_model_tag: string | null;
  /** Phase 3b: GGUF slot slug (e.g. "gemma4-e4b-it-q5_k_m") when
   * engine=embedded. null = global default. */
  embedded_llm_slot: string | null;
  /** Phase 3b: Whisper slot slug per mode. null = global default. */
  whisper_model_slot: string | null;
  /** Phase 3b: Whisper initial prompt (glossary / proper-name hints). */
  initial_prompt: string | null;
  injection_method: InjectionMethod;
  language: string | null;
  system_prompt: string | null;
  /** Phase 1: per-mode sampling params. */
  temperature: number | null;
  top_p: number | null;
  repeat_penalty: number | null;
  /** Phase 3b: LLM output token limit. */
  max_tokens: number | null;
}

export interface Settings {
  audio_input_device: string | null;
  whisper_model_path: string | null;
  whisper_default_slot: string;
  autostart: boolean;
  ollama_url: string;
  /** Phase 1: Ollama duration string ("5m", "0", "-1"). */
  ollama_keep_alive: string;
  /** Phase 3b: which GGUF model is loaded on the first embedded LLM
   * call. */
  llm_default_slot: string;
  /** Phase 3b: optional override path to a custom GGUF file. */
  llm_model_path: string | null;
  onboarding_done: boolean;
  whisper_n_threads: number | null;
  menu_hotkey: string;
  last_selected_mode_id: string | null;
  /**
   * UI locale (BCP-47). `null` = never set — the backend fills this
   * on first start from the OS locale. The frontend maps the value
   * via `pickSupported()` onto one of the supported languages
   * [en, de, fr, es, it].
   */
  locale: string | null;
}

export interface LogLine {
  timestamp: string;
  level: "trace" | "debug" | "info" | "warn" | "error";
  target: string;
  message: string;
}
