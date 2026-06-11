// SPDX-License-Identifier: GPL-3.0-or-later
// Zustand stores. Kept small: one store per domain, side effects via
// async actions, no intermediate layers.

import { create } from "zustand";
import type { Mode, Settings } from "../lib/types";
import {
  ipcGetModes,
  ipcGetSettings,
  ipcListAudioDevices,
  ipcSetSettings,
  retryWhileUnmanaged,
} from "../lib/tauri";
import {
  applyTheme,
  readStoredChoice,
  storeChoice,
  type ThemeChoice,
} from "../lib/theme";

type Tab = "settings" | "modes" | "logs";

interface UIState {
  activeTab: Tab;
  setActiveTab: (tab: Tab) => void;
  theme: ThemeChoice;
  setTheme: (theme: ThemeChoice) => void;
}

export const useUIStore = create<UIState>((set) => ({
  // Default tab "modes": the typical power user opens the main window
  // to trigger or edit a mode — configuration is secondary.
  activeTab: "modes",
  setActiveTab: (activeTab) => set({ activeTab }),
  theme: readStoredChoice(),
  setTheme: (theme) => {
    storeChoice(theme);
    applyTheme(theme);
    set({ theme });
  },
}));

interface SettingsState {
  settings: Settings | null;
  audioDevices: string[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  loadAudioDevices: () => Promise<void>;
  update: (partial: Partial<Settings>) => Promise<void>;
}

// Tail of the update() serialization queue (see update() below).
let updateQueue: Promise<void> = Promise.resolve();

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  audioDevices: [],
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      const settings = await retryWhileUnmanaged(ipcGetSettings);
      set({ settings, loading: false });
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },
  loadAudioDevices: async () => {
    try {
      const audioDevices = await ipcListAudioDevices();
      set({ audioDevices });
    } catch (e) {
      set({ error: `Audio devices: ${e}` });
    }
  },
  update: (partial) => {
    // Serialize updates: each call reads get().settings only after the
    // previous call's write has landed. Without this, two overlapping
    // calls read the same base and the later write drops the earlier
    // field (the IPC persist is a yield point).
    const run = async () => {
      const current = get().settings;
      if (!current) return;
      const next = { ...current, ...partial };
      try {
        await ipcSetSettings(next);
        set({ settings: next });
      } catch (e) {
        set({ error: String(e) });
      }
    };
    updateQueue = updateQueue.then(run, run);
    return updateQueue;
  },
}));

interface ModesState {
  modes: Mode[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useModesStore = create<ModesState>((set) => ({
  modes: [],
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      const modes = await retryWhileUnmanaged(ipcGetModes);
      set({ modes, loading: false });
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },
}));
