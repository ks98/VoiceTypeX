// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import {
  overlayHint,
  shownWindows,
  usageLevel,
  windowKind,
} from "./chatgptUsage";
import type { ChatGptUsage, UsageWindow } from "./tauri";

function win(used: number, minutes: number | null, resetsAt: number | null) {
  return {
    used_percent: used,
    window_minutes: minutes,
    resets_at: resetsAt,
  } satisfies UsageWindow;
}

function usage(
  primary: UsageWindow | null,
  secondary: UsageWindow | null,
): ChatGptUsage {
  return { primary, secondary, limit_reached: false, fetched_at: 0 };
}

describe("windowKind", () => {
  it("names the observed and common window lengths", () => {
    expect(windowKind(300)).toBe("5h");
    expect(windowKind(10080)).toBe("weekly");
    expect(windowKind(1440)).toBe("daily");
    expect(windowKind(290)).toBe("5h");
    expect(windowKind(120)).toBe("other");
    expect(windowKind(null)).toBe("other");
  });
});

describe("usageLevel", () => {
  it("warns from 80 % and marks 100 % as reached", () => {
    expect(usageLevel(79.9)).toBe("ok");
    expect(usageLevel(80)).toBe("warn");
    expect(usageLevel(100)).toBe("reached");
  });
});

describe("shownWindows", () => {
  it("drops missing windows and windows that already reset", () => {
    const u = usage(win(10, 300, 50), win(90, 10080, 500));
    expect(shownWindows(u, 100).map((s) => s.kind)).toEqual(["weekly"]);
    expect(shownWindows(u, 10).map((s) => s.kind)).toEqual(["5h", "weekly"]);
    expect(shownWindows(usage(null, null), 0)).toEqual([]);
    expect(shownWindows(null, 0)).toEqual([]);
  });
});

describe("overlayHint", () => {
  it("stays quiet below the threshold", () => {
    expect(overlayHint(usage(win(79, 300, null), null), 0)).toBeNull();
  });

  it("picks the fullest window at or above the threshold", () => {
    const hint = overlayHint(
      usage(win(85, 300, null), win(100, 10080, null)),
      0,
    );
    expect(hint?.kind).toBe("weekly");
    expect(hint?.level).toBe("reached");
  });
});
