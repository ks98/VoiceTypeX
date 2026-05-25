// SPDX-License-Identifier: GPL-3.0-or-later
//
// Theme control: system / light / dark.
//
// The choice is persisted to localStorage. Applied by adding/removing
// the `dark` class on <html> — Tailwind's darkMode: "class" picks up
// the class. Foundation phase: the default stays explicitly "dark" so
// nothing flips visually after the token refactor; wave 2 changes the
// default to "system" and exposes the toggle in the settings.

export type ThemeChoice = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "vtx.theme";

export function initTheme(): ThemeChoice {
  const stored = readStoredChoice();
  applyTheme(stored);
  return stored;
}

export function readStoredChoice(): ThemeChoice {
  if (typeof localStorage === "undefined") return "system";
  const v = localStorage.getItem(STORAGE_KEY);
  return v === "light" || v === "dark" || v === "system" ? v : "system";
}

export function storeChoice(choice: ThemeChoice): void {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(STORAGE_KEY, choice);
}

export function applyTheme(choice: ThemeChoice): ResolvedTheme {
  const resolved = resolve(choice);
  const root = document.documentElement;
  if (resolved === "dark") {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
  return resolved;
}

export function resolve(choice: ThemeChoice): ResolvedTheme {
  if (choice === "light" || choice === "dark") return choice;
  return systemPrefersDark() ? "dark" : "light";
}

function systemPrefersDark(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return true;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

// Observe system theme changes. The callback fires only when the
// current user choice is "system" — otherwise the explicit override
// wins. We read the choice fresh from localStorage instead of from a
// store snapshot, so there's no stale-closure bug.
export function subscribeSystemTheme(
  onChange: (resolved: ResolvedTheme) => void,
): () => void {
  if (typeof window === "undefined" || !window.matchMedia) return () => {};
  const mql = window.matchMedia("(prefers-color-scheme: dark)");
  const handler = () => {
    const stored = readStoredChoice();
    if (stored !== "system") return;
    const next: ResolvedTheme = mql.matches ? "dark" : "light";
    applyTheme("system");
    onChange(next);
  };
  mql.addEventListener("change", handler);
  return () => mql.removeEventListener("change", handler);
}
