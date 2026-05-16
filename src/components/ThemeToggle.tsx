// SPDX-License-Identifier: GPL-3.0-or-later
import { useUIStore } from "../store";
import type { ThemeChoice } from "../lib/theme";

const OPTIONS: { id: ThemeChoice; label: string; hint: string }[] = [
  { id: "system", label: "System", hint: "Folgt dem Betriebssystem" },
  { id: "light", label: "Hell", hint: "Helles Theme erzwingen" },
  { id: "dark", label: "Dunkel", hint: "Dunkles Theme erzwingen" },
];

export default function ThemeToggle(): JSX.Element {
  const theme = useUIStore((s) => s.theme);
  const setTheme = useUIStore((s) => s.setTheme);

  return (
    <div className="inline-flex rounded-md border border-outline bg-surface overflow-hidden text-xs">
      {OPTIONS.map((opt) => (
        <button
          key={opt.id}
          type="button"
          onClick={() => setTheme(opt.id)}
          title={opt.hint}
          aria-pressed={theme === opt.id}
          className={
            "px-3 py-1.5 font-medium whitespace-nowrap leading-none transition-colors " +
            (theme === opt.id
              ? "bg-brand text-brand-contrast"
              : "text-fg-muted hover:bg-elevated hover:text-fg")
          }
        >
          {opt.label}
        </button>
      ))}
    </div>
  );
}
