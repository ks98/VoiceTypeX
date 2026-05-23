// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import Sidebar from "./components/Sidebar";
import OnboardingWizard from "./components/OnboardingWizard";
import Settings from "./views/Settings";
import Modes from "./views/Modes";
import Logs from "./views/Logs";
import { useSettingsStore, useUIStore } from "./store";

export default function App(): JSX.Element {
  const activeTab = useUIStore((s) => s.activeTab);
  const setActiveTab = useUIStore((s) => s.setActiveTab);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);
  const update = useSettingsStore((s) => s.update);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  // Overlay sendet `app://focus-logs`, wenn der User auf einen Fehler im
  // Recording-Overlay klickt. Wir wechseln dann automatisch in den Logs-Tab.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen("app://focus-logs", () => setActiveTab("logs")).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, [setActiveTab]);

  // Wizard-Visibility ist seit dem Settings-Re-Trigger-Refactor reine
  // Settings-Funktion: `update({ onboarding_done: false })` öffnet ihn,
  // `update({ onboarding_done: true })` beim Wizard-Finish schließt ihn.
  const showOnboarding =
    settings !== null && settings.onboarding_done === false;

  const titleMap: Record<typeof activeTab, string> = {
    settings: "Einstellungen",
    modes: "Modi",
    logs: "Logs",
  };

  return (
    <div className="min-h-screen flex bg-canvas text-fg">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <header className="px-8 pt-6 pb-4 border-b border-outline">
          <h1 className="text-xl font-semibold tracking-tight text-fg">
            {titleMap[activeTab]}
          </h1>
          <p className="text-xs text-fg-muted mt-0.5">
            Diktiere — VoiceTypeX schreibt es.
          </p>
        </header>
        <main className="flex-1 px-8 py-6 overflow-auto">
          {activeTab === "settings" ? <Settings /> : null}
          {activeTab === "modes" ? <Modes /> : null}
          {activeTab === "logs" ? <Logs /> : null}
        </main>
      </div>
      {showOnboarding ? (
        <OnboardingWizard
          onClose={() => void update({ onboarding_done: true })}
        />
      ) : null}
    </div>
  );
}
