// SPDX-License-Identifier: GPL-3.0-or-later
//
// Theme-Steuerung: System / Hell / Dunkel.
//
// Die Wahl wird in localStorage persistiert. Anwendung erfolgt durch
// Setzen/Entfernen der `dark`-Klasse auf <html> — Tailwind's
// darkMode: "class" greift die Klasse auf. Foundation-Phase: Default
// bleibt explizit "dark", damit nach dem Token-Refactor visuell nichts
// kippt; Welle 2 ändert den Default auf "system" und exposed den
// Toggle in den Settings.

export type ThemeChoice = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "vtx.theme";

export function initTheme(): ThemeChoice {
  const stored = readStoredChoice();
  applyTheme(stored);
  return stored;
}

export function readStoredChoice(): ThemeChoice {
  if (typeof localStorage === "undefined") return "dark";
  const v = localStorage.getItem(STORAGE_KEY);
  return v === "light" || v === "dark" || v === "system" ? v : "dark";
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

// System-Theme-Wechsel beobachten. Callback feuert nur, wenn die aktuelle
// User-Wahl "system" ist — sonst gewinnt der explizite Override. Wir
// lesen die Wahl frisch aus localStorage statt aus einem Store-Snapshot,
// damit es keinen Stale-Closure-Bug gibt.
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
