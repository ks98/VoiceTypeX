// SPDX-License-Identifier: GPL-3.0-or-later
import { useUIStore } from "../store";

const TABS: { id: "settings" | "modes" | "logs"; label: string }[] = [
  { id: "settings", label: "Einstellungen" },
  { id: "modes", label: "Modi" },
  { id: "logs", label: "Logs" },
];

export default function TabBar(): JSX.Element {
  const activeTab = useUIStore((s) => s.activeTab);
  const setActiveTab = useUIStore((s) => s.setActiveTab);

  return (
    <nav className="flex border-b border-slate-800 bg-slate-950">
      {TABS.map((tab) => (
        <button
          key={tab.id}
          type="button"
          onClick={() => setActiveTab(tab.id)}
          className={
            "px-6 py-3 text-sm font-medium transition-colors " +
            (activeTab === tab.id
              ? "text-brand-500 border-b-2 border-brand-500"
              : "text-slate-400 hover:text-slate-200")
          }
        >
          {tab.label}
        </button>
      ))}
    </nav>
  );
}
