// SPDX-License-Identifier: GPL-3.0-or-later
import { useUIStore } from "../store";
import { useT } from "../i18n";
import type { ThemeChoice } from "../lib/theme";

const THEME_IDS = ["system", "light", "dark"] as const satisfies readonly ThemeChoice[];

export default function ThemeToggle(): JSX.Element {
  const t = useT();
  const theme = useUIStore((s) => s.theme);
  const setTheme = useUIStore((s) => s.setTheme);

  return (
    <div className="inline-flex rounded-md border border-outline bg-surface overflow-hidden text-xs">
      {THEME_IDS.map((id) => (
        <button
          key={id}
          type="button"
          onClick={() => setTheme(id)}
          title={t(`theme.${id}.hint`)}
          aria-pressed={theme === id}
          className={
            "px-3 py-1.5 font-medium whitespace-nowrap leading-none transition-colors " +
            (theme === id
              ? "bg-brand text-brand-contrast"
              : "text-fg-muted hover:bg-elevated hover:text-fg")
          }
        >
          {t(`theme.${id}.label`)}
        </button>
      ))}
    </div>
  );
}
