// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  engineSteps,
  errorStage,
  formatElapsed,
  hotkeyLabel,
  silenceHintDue,
  tailWords,
} from "./overlayModel";

describe("engineSteps", () => {
  it("walks STT then LLM through the pipeline phases", () => {
    expect(engineSteps("recording")).toEqual({
      stt: "pending",
      llm: "pending",
    });
    expect(engineSteps("transcribing")).toEqual({
      stt: "active",
      llm: "pending",
    });
    expect(engineSteps("postprocessing")).toEqual({
      stt: "done",
      llm: "active",
    });
    expect(engineSteps("injecting")).toEqual({ stt: "done", llm: "done" });
  });
});

describe("errorStage", () => {
  it("names the phase that failed", () => {
    expect(errorStage("transcribing")).toBe("transcribing");
    expect(errorStage("postprocessing")).toBe("postprocessing");
    expect(errorStage("idle")).toBe("generic");
    expect(errorStage(null)).toBe("generic");
  });
});

describe("formatElapsed", () => {
  it("formats minutes and zero-padded seconds", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(7_900)).toBe("0:07");
    expect(formatElapsed(65_000)).toBe("1:05");
    expect(formatElapsed(750_000)).toBe("12:30");
    expect(formatElapsed(-5)).toBe("0:00");
  });
});

describe("silenceHintDue", () => {
  it("waits four seconds and stays quiet once speech was heard", () => {
    expect(silenceHintDue(3_999, 0)).toBe(false);
    expect(silenceHintDue(4_000, 0.05)).toBe(true);
    expect(silenceHintDue(10_000, 0.4)).toBe(false);
  });
});

describe("hotkeyLabel", () => {
  it("turns accelerators into readable labels", () => {
    expect(hotkeyLabel("CommandOrControl+Alt+Space")).toBe("Ctrl+Alt+Space");
    expect(hotkeyLabel("Super+shift+d")).toBe("Super+Shift+D");
    expect(hotkeyLabel("Meta+Space")).toBe("Super+Space");
    expect(hotkeyLabel("Ctrl+Alt+Space")).toBe("Ctrl+Alt+Space");
  });
});

describe("tailWords", () => {
  it("keeps the newest words that fit, with absolute indices", () => {
    const r = tailWords("one two three four", 10);
    expect(r.words).toEqual([
      { index: 2, word: "three" },
      { index: 3, word: "four" },
    ]);
    expect(r.truncated).toBe(true);
  });

  it("returns everything when it fits", () => {
    const r = tailWords("  hello   world ", 40);
    expect(r.words.map((w) => w.word)).toEqual(["hello", "world"]);
    expect(r.truncated).toBe(false);
  });

  it("always shows at least the last word", () => {
    const r = tailWords("a supercalifragilistic", 5);
    expect(r.words).toEqual([{ index: 1, word: "supercalifragilistic" }]);
  });

  it("is empty for an empty transcript", () => {
    expect(tailWords("", 10)).toEqual({ words: [], truncated: false });
  });
});
