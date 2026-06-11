// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import type { HardwareReport, ModelDownloadProgress } from "./tauri";
import type { Mode, Settings } from "./types";

// Contract test — PAYLOAD-SHAPE parity (#49).
//
// For the four key IPC payloads (Settings, Mode, HardwareReport,
// ModelDownloadProgress) this pins the exact field set of the TS
// interface against the same canonical key list the Rust side pins in
// its own #[test]s:
//   - Settings              -> src-tauri/src/core/config.rs
//   - Mode                  -> src-tauri/src/core/modes.rs
//   - HardwareReport        -> src-tauri/src/core/hardware.rs
//   - ModelDownloadProgress -> src-tauri/src/ipc/settings.rs
//
// How this catches drift:
//   * Each `sample` is annotated `satisfies <Interface>`, so `tsc`
//     rejects a missing required field OR an excess property. A TS
//     interface field add/rename/remove therefore forces the literal to
//     change, and `Object.keys(sample)` then diverges from EXPECTED here
//     -> red test.
//   * The Rust side serializes a representative instance and asserts the
//     same EXPECTED list -> a Rust serde field add/rename/remove turns
//     that test red.
//   Both sides are anchored to identical hard-coded key lists, so a
//   ONE-SIDED change always breaks a test before it can desync at runtime.
//
// Honest limit (contract-tests-over-codegen, no specta/ts-rs): the lists
// do NOT auto-derive from the structs. A *coordinated* change to the Rust
// struct AND the Rust key list AND the TS interface AND this list would
// pass — that is the accepted trade-off of contract-tests-over-codegen.

function keysOf(o: Record<string, unknown>): string[] {
  return Object.keys(o).sort();
}

function sorted(keys: readonly string[]): string[] {
  return [...keys].sort();
}

describe("Settings payload shape", () => {
  const EXPECTED = [
    "audio_input_device",
    "whisper_model_path",
    "whisper_default_slot",
    "autostart",
    "ollama_url",
    "ollama_keep_alive",
    "llm_default_slot",
    "llm_model_path",
    "onboarding_done",
    "whisper_n_threads",
    "whisper_beam_size",
    "menu_hotkey",
    "last_selected_mode_id",
    "locale",
  ] as const;

  it("TS Settings has exactly the canonical Rust serde field set", () => {
    const sample = {
      audio_input_device: null,
      whisper_model_path: null,
      whisper_default_slot: "large-v3-turbo-q8_0",
      autostart: false,
      ollama_url: "http://127.0.0.1:11434",
      ollama_keep_alive: "5m",
      llm_default_slot: "gemma3-1b-it-q5_k_m",
      llm_model_path: null,
      onboarding_done: false,
      whisper_n_threads: null,
      whisper_beam_size: 2,
      menu_hotkey: "CommandOrControl+Alt+Space",
      last_selected_mode_id: null,
      locale: null,
    } satisfies Settings;
    expect(keysOf(sample)).toStrictEqual(sorted(EXPECTED));
  });
});

describe("Mode payload shape", () => {
  // `hotkey` is included: the Rust field carries
  // `skip_serializing_if = "Option::is_none"` but is on the wire whenever
  // a user TOML still sets it, so the contract pins the full superset.
  const EXPECTED = [
    "id",
    "name",
    "description",
    "hotkey",
    "transcription",
    "processing",
    "cloud_stt_provider",
    "whisper_model_slot",
    "initial_prompt",
    "whisper_beam_size",
    "cloud_llm_provider",
    "cloud_llm_model",
    "local_llm_model",
    "local_engine",
    "ollama_model_tag",
    "embedded_llm_slot",
    "injection_method",
    "paste_shortcut",
    "input",
    "output",
    "output_fallback",
    "language",
    "system_prompt",
    "temperature",
    "top_p",
    "repeat_penalty",
    "max_tokens",
  ] as const;

  it("TS Mode has exactly the canonical Rust serde field set", () => {
    const sample = {
      id: "m",
      name: "M",
      description: "",
      hotkey: "CommandOrControl+Alt+D",
      transcription: "local",
      processing: "none",
      cloud_stt_provider: null,
      whisper_model_slot: null,
      initial_prompt: null,
      whisper_beam_size: null,
      cloud_llm_provider: null,
      cloud_llm_model: null,
      local_llm_model: null,
      local_engine: null,
      ollama_model_tag: null,
      embedded_llm_slot: null,
      injection_method: "clipboard",
      paste_shortcut: "auto",
      input: "voice",
      output: "insert",
      output_fallback: "replace",
      language: null,
      system_prompt: null,
      temperature: null,
      top_p: null,
      repeat_penalty: null,
      max_tokens: null,
    } satisfies Mode;
    expect(keysOf(sample)).toStrictEqual(sorted(EXPECTED));
  });
});

describe("HardwareReport payload shape", () => {
  const EXPECTED = [
    "os",
    "cpu_logical_cores",
    "has_openblas",
    "has_vulkan",
    "has_nvidia_gpu",
    "has_amd_gpu",
    "is_apple_silicon",
    "total_ram_gb",
    "available_ram_gb",
    "recommended_variant",
    "recommended_speedup",
  ] as const;

  it("TS HardwareReport has exactly the canonical Rust serde field set", () => {
    const sample = {
      os: "linux",
      cpu_logical_cores: 8,
      has_openblas: false,
      has_vulkan: false,
      has_nvidia_gpu: false,
      has_amd_gpu: false,
      is_apple_silicon: false,
      total_ram_gb: 16,
      available_ram_gb: 8,
      recommended_variant: "cpu",
      recommended_speedup: 1,
    } satisfies HardwareReport;
    expect(keysOf(sample)).toStrictEqual(sorted(EXPECTED));
  });
});

describe("ModelDownloadProgress payload shape", () => {
  const EXPECTED = ["downloaded", "total"] as const;

  it("TS ModelDownloadProgress has exactly the canonical Rust serde field set", () => {
    const sample = {
      downloaded: 1,
      total: 2,
    } satisfies ModelDownloadProgress;
    expect(keysOf(sample)).toStrictEqual(sorted(EXPECTED));
  });
});
