// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { computeBlockingReasons } from "./modeValidation";
import type { Mode } from "../lib/types";

// A minimal mode that passes every gate: valid id + name, local STT,
// no post-processing, voice input, no sampling overrides. Tests clone
// and mutate one field at a time to isolate each blocking branch.
function validMode(overrides: Partial<Mode> = {}): Mode {
  return {
    id: "my-mode",
    name: "My Mode",
    description: "",
    transcription: "local",
    processing: "none",
    cloud_stt_provider: null,
    cloud_llm_provider: null,
    cloud_llm_model: null,
    local_llm_model: null,
    local_engine: null,
    ollama_model_tag: null,
    embedded_llm_slot: null,
    whisper_model_slot: null,
    initial_prompt: null,
    whisper_beam_size: null,
    injection_method: "clipboard",
    paste_shortcut: "auto",
    input: "voice",
    output: "insert",
    output_fallback: "replace",
    language: "de",
    system_prompt: null,
    temperature: null,
    top_p: null,
    repeat_penalty: null,
    max_tokens: null,
    ...overrides,
  };
}

const NOT_WINDOWS = false;
const WINDOWS = true;

describe("computeBlockingReasons — non-blocking", () => {
  it("returns no reasons for a fully-valid mode (canSave === true)", () => {
    expect(computeBlockingReasons(validMode(), NOT_WINDOWS)).toEqual([]);
  });

  it("allows a complete cloud STT + cloud LLM mode", () => {
    const mode = validMode({
      transcription: "cloud",
      cloud_stt_provider: "groq",
      processing: "cloud",
      cloud_llm_provider: "openai",
      system_prompt: "Clean up the transcript.",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toEqual([]);
  });

  it("allows a complete local-LLM (ollama) mode", () => {
    const mode = validMode({
      processing: "local",
      local_engine: "ollama",
      ollama_model_tag: "llama3.2:3b",
      system_prompt: "Clean up the transcript.",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toEqual([]);
  });

  it("allows a local-LLM embedded mode without an ollama tag", () => {
    const mode = validMode({
      processing: "local",
      local_engine: "embedded",
      system_prompt: "Clean up the transcript.",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toEqual([]);
  });

  it("allows sampling values at the boundary of their ranges", () => {
    const mode = validMode({
      processing: "local",
      local_engine: "embedded",
      system_prompt: "x",
      temperature: 2,
      top_p: 0,
      repeat_penalty: 0.5,
      max_tokens: 8192,
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toEqual([]);
  });
});

describe("computeBlockingReasons — id", () => {
  it("blocks on an empty id", () => {
    const reasons = computeBlockingReasons(validMode({ id: "" }), NOT_WINDOWS);
    expect(reasons).toContain("id_missing");
    expect(reasons).not.toContain("id_invalid");
  });

  it("blocks on an id with illegal characters", () => {
    const reasons = computeBlockingReasons(
      validMode({ id: "bad id!" }),
      NOT_WINDOWS,
    );
    expect(reasons).toContain("id_invalid");
    expect(reasons).not.toContain("id_missing");
  });
});

describe("computeBlockingReasons — name", () => {
  it("blocks on an empty name", () => {
    expect(
      computeBlockingReasons(validMode({ name: "" }), NOT_WINDOWS),
    ).toContain("name_missing");
  });
});

describe("computeBlockingReasons — cloud providers", () => {
  it("blocks cloud STT without a provider", () => {
    const mode = validMode({
      transcription: "cloud",
      cloud_stt_provider: null,
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain(
      "cloud_stt_provider",
    );
  });

  it("does not require a cloud STT provider for local STT", () => {
    const mode = validMode({
      transcription: "local",
      cloud_stt_provider: null,
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).not.toContain(
      "cloud_stt_provider",
    );
  });

  it("blocks cloud LLM without a provider", () => {
    const mode = validMode({
      processing: "cloud",
      cloud_llm_provider: null,
      system_prompt: "x",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain(
      "cloud_llm_provider",
    );
  });
});

describe("computeBlockingReasons — ollama tag", () => {
  it("blocks a local ollama mode without a tag", () => {
    const mode = validMode({
      processing: "local",
      local_engine: "ollama",
      ollama_model_tag: null,
      system_prompt: "x",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain("ollama_tag");
  });

  it("does not require an ollama tag for the embedded engine", () => {
    const mode = validMode({
      processing: "local",
      local_engine: "embedded",
      ollama_model_tag: null,
      system_prompt: "x",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).not.toContain(
      "ollama_tag",
    );
  });

  it("forces ollama on Windows: a tag-less local mode is blocked even with engine=embedded", () => {
    // On Windows the embedded engine does not exist (issue #1); the
    // editor coerces local-LLM modes to ollama, so a missing tag blocks.
    const mode = validMode({
      processing: "local",
      local_engine: "embedded",
      ollama_model_tag: null,
      system_prompt: "x",
    });
    expect(computeBlockingReasons(mode, WINDOWS)).toContain("ollama_tag");
  });
});

describe("computeBlockingReasons — selection input", () => {
  it("blocks selection input with no post-processing", () => {
    const mode = validMode({ input: "selection", processing: "none" });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain(
      "selection_needs_llm",
    );
  });

  it("allows selection input once a processing LLM is configured", () => {
    const mode = validMode({
      input: "selection",
      processing: "cloud",
      cloud_llm_provider: "openai",
      system_prompt: "x",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).not.toContain(
      "selection_needs_llm",
    );
  });
});

describe("computeBlockingReasons — system prompt", () => {
  it("blocks a processing mode with a null system prompt", () => {
    const mode = validMode({
      processing: "cloud",
      cloud_llm_provider: "openai",
      system_prompt: null,
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain("system_prompt");
  });

  it("blocks a processing mode with an empty system prompt", () => {
    const mode = validMode({
      processing: "cloud",
      cloud_llm_provider: "openai",
      system_prompt: "",
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain("system_prompt");
  });

  it("does not require a system prompt when processing is none", () => {
    const mode = validMode({ processing: "none", system_prompt: null });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).not.toContain(
      "system_prompt",
    );
  });
});

describe("computeBlockingReasons — sampling", () => {
  it.each([
    ["temperature", { temperature: 2.1 }],
    ["temperature", { temperature: -0.1 }],
    ["top_p", { top_p: 1.1 }],
    ["repeat_penalty", { repeat_penalty: 0.4 }],
    ["repeat_penalty", { repeat_penalty: 2.5 }],
    ["max_tokens", { max_tokens: 0 }],
    ["max_tokens", { max_tokens: 9000 }],
  ])("blocks an out-of-range %s value", (_label, override) => {
    const mode = validMode({
      processing: "local",
      local_engine: "embedded",
      system_prompt: "x",
      ...override,
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain("sampling");
  });

  it("blocks non-finite sampling values", () => {
    const mode = validMode({
      processing: "local",
      local_engine: "embedded",
      system_prompt: "x",
      temperature: Number.NaN,
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toContain("sampling");
  });
});

describe("computeBlockingReasons — order & accumulation", () => {
  it("preserves display order across multiple violations", () => {
    const mode = validMode({
      id: "",
      name: "",
      transcription: "cloud",
      cloud_stt_provider: null,
    });
    expect(computeBlockingReasons(mode, NOT_WINDOWS)).toEqual([
      "id_missing",
      "name_missing",
      "cloud_stt_provider",
    ]);
  });
});
