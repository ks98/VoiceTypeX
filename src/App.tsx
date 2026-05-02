// SPDX-License-Identifier: GPL-3.0-or-later
import TabBar from "./components/TabBar";
import Settings from "./views/Settings";
import Modes from "./views/Modes";
import Logs from "./views/Logs";
import { useUIStore } from "./store";

export default function App(): JSX.Element {
  const activeTab = useUIStore((s) => s.activeTab);

  return (
    <div className="min-h-screen flex flex-col bg-slate-950 text-slate-100">
      <header className="px-6 pt-5 pb-3">
        <h1 className="text-2xl font-bold text-brand-500">VoiceTypeX</h1>
        <p className="text-xs text-slate-500">
          Diktiere — VoiceTypeX schreibt es. Phase 1 (lokales Diktat).
        </p>
      </header>
      <TabBar />
      <main className="flex-1 p-6 overflow-auto">
        {activeTab === "settings" ? <Settings /> : null}
        {activeTab === "modes" ? <Modes /> : null}
        {activeTab === "logs" ? <Logs /> : null}
      </main>
    </div>
  );
}
