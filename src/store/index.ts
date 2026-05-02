// SPDX-License-Identifier: GPL-3.0-or-later
// Zustand-Stores. Klein gehalten: ein Store pro Domäne, Side-Effects via
// async Actions, keine Mittler-Schichten.

import { create } from "zustand";
import type { Mode, Settings } from "../lib/types";
import {
  ipcGetModes,
  ipcGetSettings,
  ipcListAudioDevices,
  ipcSetSettings,
} from "../lib/tauri";

type Tab = "settings" | "modes" | "logs";

interface UIState {
  activeTab: Tab;
  setActiveTab: (tab: Tab) => void;
}

export const useUIStore = create<UIState>((set) => ({
  activeTab: "settings",
  setActiveTab: (activeTab) => set({ activeTab }),
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

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  audioDevices: [],
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      const settings = await ipcGetSettings();
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
      set({ error: `Audio-Geraete: ${e}` });
    }
  },
  update: async (partial) => {
    const current = get().settings;
    if (!current) return;
    const next = { ...current, ...partial };
    try {
      await ipcSetSettings(next);
      set({ settings: next });
    } catch (e) {
      set({ error: String(e) });
    }
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
      const modes = await ipcGetModes();
      set({ modes, loading: false });
    } catch (e) {
      set({ loading: false, error: String(e) });
    }
  },
}));
