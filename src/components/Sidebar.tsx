// SPDX-License-Identifier: GPL-3.0-or-later
import { useUIStore } from "../store";
import Logo from "./Logo";

const TABS = [
  { id: "settings", label: "Einstellungen" },
  { id: "modes", label: "Modi" },
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
          <div className="text-[11px] text-fg-faint leading-tight">
            Diktat in jede App
          </div>
        </div>
      </div>
      {TABS.map((tab) => (
        <button
          key={tab.id}
          type="button"
          onClick={() => setActiveTab(tab.id)}
          className={
            "px-3 py-2 rounded-md text-sm text-left whitespace-nowrap transition-colors " +
            (activeTab === tab.id
              ? "bg-elevated text-fg font-medium"
              : "text-fg-muted hover:text-fg hover:bg-elevated/60")
          }
        >
          {tab.label}
        </button>
      ))}
    </nav>
  );
}
