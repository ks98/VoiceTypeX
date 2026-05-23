// SPDX-License-Identifier: GPL-3.0-or-later
import { useUIStore } from "../store";
import Logo from "./Logo";
import ThemeToggle from "./ThemeToggle";

// Reihenfolge nach Nutzungs-Frequenz: Power-User oeffnet das Hauptfenster
// meistens um Modi zu triggern oder zu editieren, nicht zum Konfigurieren.
const TABS = [
  { id: "modes", label: "Modi" },
  { id: "settings", label: "Einstellungen" },
  { id: "logs", label: "Logs" },
] as const;

export default function Sidebar(): JSX.Element {
  const activeTab = useUIStore((s) => s.activeTab);
  const setActiveTab = useUIStore((s) => s.setActiveTab);

  return (
    <nav className="w-52 shrink-0 border-r border-outline bg-surface flex flex-col py-4 px-3 gap-0.5">
      <div className="px-2 pb-3 mb-1 border-b border-outline flex items-center gap-2.5">
        <Logo className="h-7 w-7 text-brand shrink-0" />
        <div className="min-w-0">
          <div className="text-sm font-semibold tracking-tight text-fg leading-tight">
            VoiceTypeX
          </div>
          <div className="text-xxs text-fg-faint leading-tight">
            Diktat in jede App
          </div>
        </div>
      </div>
      {TABS.map((tab) => (
        <button
          key={tab.id}
          type="button"
          onClick={() => setActiveTab(tab.id)}
          aria-current={activeTab === tab.id ? "page" : undefined}
          className={
            "px-3 py-2 rounded-md text-sm text-left whitespace-nowrap transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 " +
            (activeTab === tab.id
              ? "bg-elevated text-fg font-medium"
              : "text-fg-muted hover:text-fg hover:bg-elevated/60")
          }
        >
          {tab.label}
        </button>
      ))}
      {/* Theme-Toggle in den Sidebar-Footer — gehoert nicht zur App-
          Konfiguration, sondern ist persoenliche Praeferenz. */}
      <div className="mt-auto pt-3 border-t border-outline">
        <ThemeToggle />
      </div>
    </nav>
  );
}
