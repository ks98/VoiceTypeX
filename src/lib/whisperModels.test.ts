// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  WHISPER_MODELS,
  whisperModelBySlot,
  recommendWhisperSlot,
} from "./whisperModels";
import type { HardwareReport } from "./tauri";

// Minimal HardwareReport — only `total_ram_gb` is read by the
// recommender; the rest is filled with inert defaults.
function hw(totalRamGb: number): HardwareReport {
  return {
    os: "linux",
    cpu_logical_cores: 8,
    has_openblas: false,
    has_vulkan: false,
    has_nvidia_gpu: false,
    has_amd_gpu: false,
    is_apple_silicon: false,
    total_ram_gb: totalRamGb,
    available_ram_gb: totalRamGb,
    recommended_variant: "cpu",
    recommended_speedup: 1,
  };
}

describe("whisperModelBySlot", () => {
  it("returns the meta for a known slot", () => {
    const meta = whisperModelBySlot("large-v3-turbo-german-q8_0");
    expect(meta).toBeDefined();
    expect(meta?.slot).toBe("large-v3-turbo-german-q8_0");
    expect(meta?.german).toBe(true);
    expect(meta?.sizeMb).toBe(874);
  });

  it("resolves the frugal fallback slot", () => {
    expect(whisperModelBySlot("small-q5_1")?.speed).toBe(5);
  });

  it("returns undefined for an unknown slot", () => {
    expect(whisperModelBySlot("does-not-exist")).toBeUndefined();
  });

  it("every slug in WHISPER_MODELS round-trips through the lookup", () => {
    for (const m of WHISPER_MODELS) {
      expect(whisperModelBySlot(m.slot)).toBe(m);
    }
  });
});

describe("recommendWhisperSlot", () => {
  it("assumes a modern machine when RAM detection is unavailable (0)", () => {
    expect(recommendWhisperSlot(hw(0), false)).toBe("large-v3-turbo-q8_0");
    expect(recommendWhisperSlot(hw(0), true)).toBe(
      "large-v3-turbo-german-q8_0",
    );
  });

  it("treats a null hardware report as no detection (Q8 sweet-spot)", () => {
    expect(recommendWhisperSlot(null, false)).toBe("large-v3-turbo-q8_0");
    expect(recommendWhisperSlot(null, true)).toBe(
      "large-v3-turbo-german-q8_0",
    );
  });

  it("picks the small fallback below 5 GB", () => {
    expect(recommendWhisperSlot(hw(4), false)).toBe("small-q5_1");
    // small-q5_1 has no German variant: preferGerman is ignored here.
    expect(recommendWhisperSlot(hw(4), true)).toBe("small-q5_1");
  });

  it("picks the lighter Q5 quant in the 5–8 GB band", () => {
    expect(recommendWhisperSlot(hw(5), false)).toBe("large-v3-turbo-q5_0");
    expect(recommendWhisperSlot(hw(6), true)).toBe(
      "large-v3-turbo-german-q5_0",
    );
    expect(recommendWhisperSlot(hw(7.9), false)).toBe("large-v3-turbo-q5_0");
  });

  it("picks the Q8 sweet-spot at and above 8 GB", () => {
    expect(recommendWhisperSlot(hw(8), false)).toBe("large-v3-turbo-q8_0");
    expect(recommendWhisperSlot(hw(8), true)).toBe(
      "large-v3-turbo-german-q8_0",
    );
    expect(recommendWhisperSlot(hw(32), false)).toBe("large-v3-turbo-q8_0");
  });
});
