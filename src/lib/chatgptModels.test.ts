// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  CHATGPT_DEFAULT_MODEL,
  CHATGPT_MODELS,
  isListedChatGptModel,
} from "./chatgptModels";

describe("chatgpt model list", () => {
  it("pins the default to the backend's DEFAULT_MODEL and lists it", () => {
    // Same literal as the Rust test `default_model_is_pinned`.
    expect(CHATGPT_DEFAULT_MODEL).toBe("gpt-6-luna");
    expect(isListedChatGptModel(CHATGPT_DEFAULT_MODEL)).toBe(true);
  });

  it("has unique slugs", () => {
    const slugs = CHATGPT_MODELS.map((m) => m.slug);
    expect(new Set(slugs).size).toBe(slugs.length);
  });

  it("treats empty, retired and foreign models as unlisted", () => {
    expect(isListedChatGptModel(null)).toBe(false);
    expect(isListedChatGptModel("gpt-5.5")).toBe(false);
    expect(isListedChatGptModel("grok-4-fast-non-reasoning")).toBe(false);
  });
});
