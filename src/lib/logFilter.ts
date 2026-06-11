// SPDX-License-Identifier: GPL-3.0-or-later

export type LevelFilter = "all" | "warn" | "error";

// Log lines come from the Rust ring buffer in the format
// `[LEVEL] target - message`, where LEVEL is padded to 5 chars
// (e.g. `INFO `). We extract the level for the filter directly at
// the start of the line.
export function extractLevel(line: string): string | null {
  if (line.length < 7 || line[0] !== "[") return null;
  const close = line.indexOf("]");
  if (close < 0) return null;
  return line.slice(1, close).trim().toUpperCase();
}

export function matchesFilter(line: string, filter: LevelFilter): boolean {
  if (filter === "all") return true;
  const level = extractLevel(line);
  if (filter === "error") return level === "ERROR";
  if (filter === "warn") return level === "WARN" || level === "ERROR";
  return true;
}
