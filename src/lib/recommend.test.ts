// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { recommendLlmSlot } from "./recommend";

describe("recommendLlmSlot", () => {
  it("returns the light default when detection is unavailable (0 GB)", () => {
    expect(recommendLlmSlot(0)).toEqual({
      slot: "gemma3-1b-it-q5_k_m",
      label: "Gemma 3 1B (Light)",
    });
  });

  it("returns the light default for negative RAM (defensive)", () => {
    expect(recommendLlmSlot(-4).slot).toBe("gemma3-1b-it-q5_k_m");
  });

  it("recommends the light slot below 8 GB", () => {
    expect(recommendLlmSlot(4).slot).toBe("gemma3-1b-it-q5_k_m");
    expect(recommendLlmSlot(7.9).slot).toBe("gemma3-1b-it-q5_k_m");
  });

  it("recommends the mid slot at the 8 GB boundary", () => {
    expect(recommendLlmSlot(8)).toEqual({
      slot: "gemma4-e2b-it-q5_k_m",
      label: "Gemma 4 E2B (Mid)",
    });
  });

  it("recommends the mid slot in the 8–12 GB band", () => {
    expect(recommendLlmSlot(11.9).slot).toBe("gemma4-e2b-it-q5_k_m");
  });

  it("recommends the pro slot at the 12 GB boundary", () => {
    expect(recommendLlmSlot(12)).toEqual({
      slot: "gemma4-e4b-it-q5_k_m",
      label: "Gemma 4 E4B (Pro)",
    });
  });

  it("recommends the pro slot above 12 GB", () => {
    expect(recommendLlmSlot(32).slot).toBe("gemma4-e4b-it-q5_k_m");
  });
});
