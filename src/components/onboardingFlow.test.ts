// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  applyChatGptChoice,
  effectiveSttChoice,
  wizardSteps,
} from "./onboardingFlow";
import type { Mode } from "../lib/types";

function mode(overrides: Partial<Mode>): Mode {
  return {
    id: "email",
    name: "Email",
    description: "Polished email.",
    transcription: "cloud",
    processing: "cloud",
    cloud_stt_provider: "xai",
    cloud_llm_provider: "xai",
    cloud_llm_model: "grok-4-fast-non-reasoning",
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
    system_prompt: "Write an email.",
    temperature: null,
    top_p: null,
    repeat_penalty: null,
    max_tokens: null,
    ...overrides,
  };
}

const noDescription = () => null;

describe("wizardSteps", () => {
  it("skips the key and local-LLM steps only when ChatGPT also post-processes", () => {
    expect(wizardSteps("local", true)).toEqual([1, 2, 3, 4, 5]);
    expect(wizardSteps("chatgpt", false)).toEqual([1, 2, 3, 4, 5]);
    expect(wizardSteps("chatgpt", true)).toEqual([1, 2, 5]);
  });
});

describe("effectiveSttChoice", () => {
  it("falls back to local until the ChatGPT sign-in succeeded", () => {
    expect(effectiveSttChoice("chatgpt", false)).toBe("local");
    expect(effectiveSttChoice("chatgpt", true)).toBe("chatgpt");
    expect(effectiveSttChoice("local", true)).toBe("local");
  });
});

describe("applyChatGptChoice", () => {
  it("moves an xAI mode to ChatGPT for STT and, with llm, for post-processing", () => {
    const next = applyChatGptChoice(mode({}), {
      llm: true,
      describe: noDescription,
    });
    expect(next).toMatchObject({
      cloud_stt_provider: "chatgpt",
      cloud_llm_provider: "chatgpt",
      cloud_llm_model: null,
      description: "Polished email.",
    });
  });

  it("keeps xAI post-processing when llm is off", () => {
    const next = applyChatGptChoice(mode({}), {
      llm: false,
      describe: noDescription,
    });
    expect(next?.cloud_stt_provider).toBe("chatgpt");
    expect(next?.cloud_llm_provider).toBe("xai");
    expect(next?.cloud_llm_model).toBe("grok-4-fast-non-reasoning");
  });

  it("turns the local exact mode into cloud STT and updates its description", () => {
    const exact = mode({
      id: "exact",
      description: "Local, no network.",
      transcription: "local",
      processing: "none",
      cloud_stt_provider: null,
      cloud_llm_provider: null,
      cloud_llm_model: null,
      whisper_model_slot: "large-v3-turbo-q8",
      system_prompt: null,
    });
    const next = applyChatGptChoice(exact, {
      llm: true,
      describe: (kind) => (kind === "exact" ? "Via ChatGPT." : null),
    });
    expect(next).toMatchObject({
      transcription: "cloud",
      cloud_stt_provider: "chatgpt",
      processing: "none",
      whisper_model_slot: null,
      description: "Via ChatGPT.",
    });
  });

  it("moves the local correction LLM to ChatGPT only with llm", () => {
    const correction = mode({
      id: "correction",
      transcription: "local",
      processing: "local",
      cloud_stt_provider: null,
      cloud_llm_provider: null,
      cloud_llm_model: null,
      local_engine: "embedded",
    });
    const describe = (_kind: string, next: Mode) =>
      next.processing === "cloud" ? "all online" : "speech online";
    const withLlm = applyChatGptChoice(correction, { llm: true, describe });
    expect(withLlm).toMatchObject({
      processing: "cloud",
      cloud_llm_provider: "chatgpt",
      local_engine: null,
      description: "all online",
    });
    const withoutLlm = applyChatGptChoice(correction, {
      llm: false,
      describe,
    });
    expect(withoutLlm).toMatchObject({
      processing: "local",
      local_engine: "embedded",
      description: "speech online",
    });
  });

  it("recognises the German default ids", () => {
    const exakt = mode({
      id: "exakt",
      transcription: "local",
      processing: "none",
      cloud_stt_provider: null,
    });
    const next = applyChatGptChoice(exakt, {
      llm: true,
      describe: (kind) => kind,
    });
    expect(next?.cloud_stt_provider).toBe("chatgpt");
    expect(next?.description).toBe("exact");
  });

  it("leaves customised and user-created modes alone", () => {
    const groq = mode({
      cloud_stt_provider: "groq",
      cloud_llm_provider: "anthropic",
    });
    expect(
      applyChatGptChoice(groq, { llm: true, describe: noDescription }),
    ).toBeNull();
    const own = mode({ id: "my-own-mode" });
    expect(
      applyChatGptChoice(own, { llm: true, describe: noDescription }),
    ).toBeNull();
  });
});
