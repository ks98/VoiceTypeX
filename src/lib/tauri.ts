// SPDX-License-Identifier: GPL-3.0-or-later
// Dünner Wrapper über Tauris invoke(), mit Fehler-Normalisierung
// und IPC-Command-Namen als zentralem Punkt.

import { invoke } from "@tauri-apps/api/core";
import type { Mode, Settings } from "./types";

export async function ipcGetSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function ipcSetSettings(settings: Settings): Promise<void> {
  return invoke("set_settings", { settings });
}

export async function ipcListAudioDevices(): Promise<string[]> {
  return invoke<string[]>("list_audio_devices");
}

export async function ipcSetWhisperModelPath(path: string): Promise<void> {
  return invoke("set_whisper_model_path", { path });
}

export async function ipcDownloadDefaultModel(): Promise<string> {
  return invoke<string>("download_default_model");
}

export interface ModelDownloadProgress {
  downloaded: number;
  total: number | null;
}

export async function ipcGetModes(): Promise<Mode[]> {
  return invoke<Mode[]>("get_modes");
}

export async function ipcReloadModes(): Promise<Mode[]> {
  return invoke<Mode[]>("reload_modes");
}

export async function ipcCreateMode(mode: Mode): Promise<void> {
  return invoke("create_mode", { mode });
}

export async function ipcUpdateMode(mode: Mode): Promise<void> {
  return invoke("update_mode", { mode });
}

export async function ipcDeleteMode(id: string): Promise<void> {
  return invoke("delete_mode", { id });
}

export async function ipcGetAppVersion(): Promise<string> {
  return invoke<string>("get_app_version");
}

export async function ipcGetRecentLogs(limit: number): Promise<string[]> {
  return invoke<string[]>("get_recent_logs", { limit });
}

export interface ProviderStatus {
  provider: string;
  configured: boolean;
}

export interface TestTranscriptionResult {
  rtf: number;
  text: string;
  audio_seconds: number;
  processing_ms: number;
}

export async function ipcRunTestTranscription(
  seconds: number,
): Promise<TestTranscriptionResult> {
  return invoke<TestTranscriptionResult>("run_test_transcription", { seconds });
}

export async function ipcGetProviderStatus(): Promise<ProviderStatus[]> {
  return invoke<ProviderStatus[]>("get_provider_status");
}

export async function ipcSetProviderKey(
  provider: string,
  key: string,
): Promise<void> {
  return invoke("set_provider_key", { provider, key });
}

export async function ipcDeleteProviderKey(provider: string): Promise<void> {
  return invoke("delete_provider_key", { provider });
}

export async function ipcTestProviderConnection(
  provider: string,
): Promise<void> {
  return invoke("test_provider_connection", { provider });
}
