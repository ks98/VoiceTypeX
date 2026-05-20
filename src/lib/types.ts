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
  local_llm_model: string | null;
  /** Phase 3b: "embedded" | "ollama" (Default = ollama bei null). */
  local_engine: string | null;
  injection_method: InjectionMethod;
  language: string | null;
  system_prompt: string | null;
  /** Phase 1: Sampling-Params pro Modus. */
  temperature: number | null;
  top_p: number | null;
  repeat_penalty: number | null;
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
