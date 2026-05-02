// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import TabBar from "./components/TabBar";
import OnboardingWizard from "./components/OnboardingWizard";
import Settings from "./views/Settings";
import Modes from "./views/Modes";
import Logs from "./views/Logs";
import { useSettingsStore, useUIStore } from "./store";

export default function App(): JSX.Element {
  const activeTab = useUIStore((s) => s.activeTab);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);
  const [forceOnboarding, setForceOnboarding] = useState(false);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  const showOnboarding =
    forceOnboarding ||
    (settings !== null && settings.onboarding_done === false);

  return (
    <div className="min-h-screen flex flex-col bg-slate-950 text-slate-100">
      <header className="px-6 pt-5 pb-3 flex justify-between items-end">
        <div>
          <h1 className="text-2xl font-bold text-brand-500">VoiceTypeX</h1>
          <p className="text-xs text-slate-500">
            Diktiere — VoiceTypeX schreibt es.
          </p>
        </div>
        {settings?.onboarding_done ? (
          <button
            type="button"
            onClick={() => setForceOnboarding(true)}
            className="text-xs text-slate-500 hover:text-slate-300"
          >
            Setup wiederholen
          </button>
        ) : null}
      </header>
      <TabBar />
      <main className="flex-1 p-6 overflow-auto">
        {activeTab === "settings" ? <Settings /> : null}
        {activeTab === "modes" ? <Modes /> : null}
        {activeTab === "logs" ? <Logs /> : null}
      </main>
      {showOnboarding ? (
        <OnboardingWizard onClose={() => setForceOnboarding(false)} />
      ) : null}
    </div>
  );
}
