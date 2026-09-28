// SPDX-License-Identifier: GPL-3.0-or-later
import type { ChatGptUsage, UsageWindow } from "./tauri";

export type WindowKind = "5h" | "daily" | "weekly" | "other";
export type UsageLevel = "ok" | "warn" | "reached";

/** One calm signal instead of a ladder: amber from here, red at 100 %. */
export const USAGE_WARN_PERCENT = 80;

const KNOWN_WINDOWS: readonly [WindowKind, number][] = [
  ["5h", 300],
  ["daily", 1440],
  ["weekly", 10080],
];

/** Label for a window length, matched within ±5 % like the Codex CLI. */
export function windowKind(minutes: number | null): WindowKind {
  if (minutes === null) return "other";
  const hit = KNOWN_WINDOWS.find(([, m]) => Math.abs(minutes - m) <= m * 0.05);
  return hit ? hit[0] : "other";
}

export function usageLevel(percent: number): UsageLevel {
  if (percent >= 100) return "reached";
  if (percent >= USAGE_WARN_PERCENT) return "warn";
  return "ok";
}

export interface ShownWindow {
  kind: WindowKind;
  window: UsageWindow;
  level: UsageLevel;
}

/**
 * Windows worth showing: reported and not yet reset — an old snapshot
 * must not claim a limit that has already rolled over.
 */
export function shownWindows(
  usage: ChatGptUsage | null,
  nowSec: number,
): ShownWindow[] {
  if (!usage) return [];
  return [usage.primary, usage.secondary]
    .filter(
      (w): w is UsageWindow =>
        w !== null && (w.resets_at === null || w.resets_at > nowSec),
    )
    .map((w) => ({
      kind: windowKind(w.window_minutes),
      window: w,
      level: usageLevel(w.used_percent),
    }));
}

/** The fullest window at or above the warning threshold, for the overlay. */
export function overlayHint(
  usage: ChatGptUsage | null,
  nowSec: number,
): ShownWindow | null {
  return shownWindows(usage, nowSec)
    .filter((s) => s.level !== "ok")
    .reduce<ShownWindow | null>(
      (worst, s) =>
        !worst || s.window.used_percent > worst.window.used_percent ? s : worst,
      null,
    );
}
