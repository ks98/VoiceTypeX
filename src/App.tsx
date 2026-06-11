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
import { EVENTS } from "./lib/events";
import { useT } from "./i18n";

export default function App(): JSX.Element {
  const t = useT();
  const activeTab = useUIStore((s) => s.activeTab);
  const setActiveTab = useUIStore((s) => s.setActiveTab);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);
  const update = useSettingsStore((s) => s.update);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  // The overlay emits `app://focus-logs` when the user clicks an
  // error in the recording overlay. We then switch automatically to
  // the Logs tab.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen(EVENTS.FOCUS_LOGS, () => setActiveTab("logs")).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, [setActiveTab]);

  // Wizard visibility has been a pure settings concern since the
  // settings re-trigger refactor: `update({ onboarding_done: false })`
  // opens it, `update({ onboarding_done: true })` on wizard finish
  // closes it.
  const showOnboarding =
    settings !== null && settings.onboarding_done === false;

  return (
    // `h-screen` instead of `min-h-screen`: with long views (Settings
    // has ~1100 lines) min-h-screen would let the outer container grow
    // with the content — then the whole page scrolls instead of just
    // `<main>`, and the sidebar including ThemeToggle (mt-auto at the
    // sidebar's end) disappears from the viewport. With a fixed height
    // `overflow-auto` applies to `<main>` and the sidebar stays visible.
    <div className="h-screen flex bg-canvas text-fg">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <header className="px-8 pt-6 pb-4 border-b border-outline">
          <h1 className="text-xl font-semibold tracking-tight text-fg">
            {t(`app.tabs.${activeTab}`)}
          </h1>
          <p className="text-xs text-fg-muted mt-0.5">{t("app.tagline")}</p>
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
