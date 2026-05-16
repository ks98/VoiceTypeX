// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import Sidebar from "./components/Sidebar";
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

  const titleMap: Record<typeof activeTab, string> = {
    settings: "Einstellungen",
    modes: "Modi",
    logs: "Logs",
  };

  return (
    <div className="min-h-screen flex bg-canvas text-fg">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <header className="px-8 pt-6 pb-4 flex justify-between items-start border-b border-outline">
          <div>
            <h1 className="text-xl font-semibold tracking-tight text-fg">
              {titleMap[activeTab]}
            </h1>
            <p className="text-xs text-fg-muted mt-0.5">
              Diktiere — VoiceTypeX schreibt es.
            </p>
          </div>
          {settings?.onboarding_done ? (
            <button
              type="button"
              onClick={() => setForceOnboarding(true)}
              className="text-xs text-fg-faint hover:text-fg-muted transition-colors"
            >
              Setup wiederholen
            </button>
          ) : null}
        </header>
        <main className="flex-1 px-8 py-6 overflow-auto">
          {activeTab === "settings" ? <Settings /> : null}
          {activeTab === "modes" ? <Modes /> : null}
          {activeTab === "logs" ? <Logs /> : null}
        </main>
      </div>
      {showOnboarding ? (
        <OnboardingWizard onClose={() => setForceOnboarding(false)} />
      ) : null}
    </div>
  );
}
