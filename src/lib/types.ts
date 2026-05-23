// SPDX-License-Identifier: GPL-3.0-or-later
// TypeScript-Spiegel der Rust-Datentypen.
// Konvention: snake_case Felder, weil das Backend serde-Defaults nutzt.

export type TranscriptionTarget = "local" | "cloud";
export type ProcessingTarget = "none" | "local" | "cloud";
export type InjectionMethod = "clipboard" | "keystrokes";

export interface Mode {
  id: string;
  name: string;
  description: string;
  /** Legacy-Feld; seit dem Menue-Hotkey-Umbau optional und ungenutzt. */
  hotkey?: string | null;
  transcription: TranscriptionTarget;
  processing: ProcessingTarget;
  cloud_stt_provider: string | null;
  cloud_llm_provider: string | null;
  cloud_llm_model: string | null;
  /**
   * @deprecated seit Phase-3b-Refactor — durch `ollama_model_tag` ersetzt.
   * Bleibt im Type-Mirror nur fuer Migration aus alten TOMLs; das Backend
   * spiegelt das Feld bei Save zurueck `ollama_model_tag` ein.
   */
  local_llm_model: string | null;
  /**
   * Engine für `processing = "local"`. `"embedded"` (Default bei null,
   * eingebauter llama-cpp-2-Pfad) oder `"ollama"` (externer Daemon, opt-in).
   * Alte TOMLs ohne explizites Feld aber mit `local_llm_model` werden
   * backend-seitig automatisch auf `"ollama"` migriert.
   */
  local_engine: string | null;
  /** Phase 3b: Ollama-Modell-Tag (z.B. "llama3.2:3b") bei engine=ollama. */
  ollama_model_tag: string | null;
  /** Phase 3b: GGUF-Slot-Slug (z.B. "gemma4-e4b-it-q5_k_m") bei engine=embedded. null = globaler Default. */
  embedded_llm_slot: string | null;
  /** Phase 3b: Whisper-Slot-Slug pro Modus. null = globaler Default. */
  whisper_model_slot: string | null;
  /** Phase 3b: Whisper-Initial-Prompt (Glossar / Eigenname-Hinweise). */
  initial_prompt: string | null;
  injection_method: InjectionMethod;
  language: string | null;
  system_prompt: string | null;
  /** Phase 1: Sampling-Params pro Modus. */
  temperature: number | null;
  top_p: number | null;
  repeat_penalty: number | null;
  /** Phase 3b: LLM-Output-Token-Limit. */
  max_tokens: number | null;
}

export interface Settings {
  audio_input_device: string | null;
  whisper_model_path: string | null;
  whisper_default_slot: string;
  diagnostic_logging: boolean;
  autostart: boolean;
  ollama_url: string;
  /** Phase 1: Ollama-Duration-String ("5m", "0", "-1"). */
  ollama_keep_alive: string;
  /** Phase 3b: Welches GGUF-Modell beim ersten Embedded-LLM-Aufruf geladen wird. */
  llm_default_slot: string;
  /** Phase 3b: Optionaler Override-Pfad zu eigenem GGUF-File. */
  llm_model_path: string | null;
  onboarding_done: boolean;
  whisper_n_threads: number | null;
  menu_hotkey: string;
  last_selected_mode_id: string | null;
}

export interface LogLine {
  timestamp: string;
  level: "trace" | "debug" | "info" | "warn" | "error";
  target: string;
  message: string;
}
