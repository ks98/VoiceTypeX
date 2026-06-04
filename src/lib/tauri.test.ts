// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { isUnmanagedStateError, retryWhileUnmanaged } from "./tauri";

// The real wording Tauri 2.x emits — LOWERCASE — verified against
// tauri-2.11.2/src/state.rs. The matcher must catch this exact casing
// (the original capital-S guess made the retry a no-op).
const REAL_TAURI_MSG =
  "state not managed for field `state` on command `get_modes`. You must call `.manage()` before using this command";

describe("isUnmanagedStateError", () => {
  it("matches Tauri's real (lowercase) unmanaged-state message", () => {
    expect(isUnmanagedStateError(REAL_TAURI_MSG)).toBe(true);
  });

  it("matches an Error instance carrying the message", () => {
    expect(
      isUnmanagedStateError(new Error(`invoke failed: ${REAL_TAURI_MSG}`)),
    ).toBe(true);
  });

  it("is case-insensitive (robust to a future re-casing)", () => {
    expect(isUnmanagedStateError("State Not Managed for field `x`")).toBe(true);
  });

  it("does not match unrelated errors", () => {
    expect(isUnmanagedStateError("network timeout")).toBe(false);
    expect(isUnmanagedStateError(null)).toBe(false);
    expect(isUnmanagedStateError(undefined)).toBe(false);
  });
});

describe("retryWhileUnmanaged", () => {
  it("retries through unmanaged-state errors, then succeeds", async () => {
    let calls = 0;
    const result = await retryWhileUnmanaged(
      async () => {
        calls += 1;
        if (calls < 3) throw new Error(REAL_TAURI_MSG);
        return "ok";
      },
      30,
      0,
    );
    expect(result).toBe("ok");
    expect(calls).toBe(3);
  });

  it("propagates a non-unmanaged error immediately (no retry)", async () => {
    let calls = 0;
    await expect(
      retryWhileUnmanaged(
        async () => {
          calls += 1;
          throw new Error("boom");
        },
        30,
        0,
      ),
    ).rejects.toThrow("boom");
    expect(calls).toBe(1);
  });

  it("gives up after the attempt budget and surfaces the error", async () => {
    let calls = 0;
    await expect(
      retryWhileUnmanaged(
        async () => {
          calls += 1;
          throw new Error(REAL_TAURI_MSG);
        },
        3,
        0,
      ),
    ).rejects.toThrow("state not managed");
    expect(calls).toBe(3);
  });
});
