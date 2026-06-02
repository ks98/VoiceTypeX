// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { isUnmanagedStateError, retryWhileUnmanaged } from "./tauri";

describe("isUnmanagedStateError", () => {
  it("matches Tauri's unmanaged-state message", () => {
    expect(
      isUnmanagedStateError(
        "State not managed for field `state` on command `get_modes`: You must call `.manage()` before using this command.",
      ),
    ).toBe(true);
  });

  it("matches an Error instance carrying the message", () => {
    expect(isUnmanagedStateError(new Error("... State not managed ..."))).toBe(
      true,
    );
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
        if (calls < 3) throw new Error("State not managed for field `state`");
        return "ok";
      },
      10,
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
        10,
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
          throw new Error("State not managed");
        },
        3,
        0,
      ),
    ).rejects.toThrow("State not managed");
    expect(calls).toBe(3);
  });
});
