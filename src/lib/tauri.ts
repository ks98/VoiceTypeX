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

export interface SessionInfo {
  display_server: "wayland" | "x11" | "windows" | "macos" | "unknown";
  global_hotkeys_supported: boolean;
  auto_paste_supported: boolean;
}

export async function ipcGetSessionInfo(): Promise<SessionInfo> {
  return invoke<SessionInfo>("get_session_info");
}

export interface WhisperBackendInfo {
  backend: "cpu" | "openblas" | "vulkan" | "cuda" | "metal" | "coreml";
  description: string;
  expected_speedup: number;
}

export async function ipcGetWhisperBackend(): Promise<WhisperBackendInfo> {
  return invoke<WhisperBackendInfo>("get_whisper_backend");
}

export interface HardwareReport {
  os: "linux" | "windows" | "macos" | "unknown";
  cpu_logical_cores: number;
  has_openblas: boolean;
  has_vulkan: boolean;
  has_nvidia_gpu: boolean;
  has_amd_gpu: boolean;
  is_apple_silicon: boolean;
  /** Gesamt-RAM in GB. 0 = Detection nicht implementiert auf diesem OS. */
  total_ram_gb: number;
  /** Aktuell verfügbares RAM in GB. 0 = nicht implementiert. */
  available_ram_gb: number;
  recommended_variant:
    | "cpu"
    | "openblas"
    | "vulkan"
    | "cuda"
    | "metal"
    | "coreml";
  recommended_speedup: number;
}

export async function ipcGetHardwareReport(): Promise<HardwareReport> {
  return invoke<HardwareReport>("get_hardware_report");
}

/**
 * Lädt das in `Settings.llm_default_slot` gewählte GGUF-LLM-Modell nach
 * `app_data_dir/models/` herunter. Progress kommt als
 * `llm-model-download-progress`-Event (separat von Whisper-Progress).
 *
 * Returns: absoluter Pfad zum heruntergeladenen GGUF-File.
 */
export async function ipcDownloadLlmDefaultModel(): Promise<string> {
  return invoke<string>("download_llm_default_model");
}

export type CachedFileKind = "whisper" | "vad" | "llm" | "partial" | "other";

export interface CachedFile {
  filename: string;
  kind: CachedFileKind;
  size_bytes: number;
}

/** Listet alle Files im `app_data_dir/models/`-Cache. */
export async function ipcListCachedFiles(): Promise<CachedFile[]> {
  return invoke<CachedFile[]>("list_cached_files");
}

/** Löscht ein einzelnes File. Returns freigegebene Bytes. */
export async function ipcDeleteCachedFile(filename: string): Promise<number> {
  return invoke<number>("delete_cached_file", { filename });
}

/** Löscht alle Modell-Files (Whisper + VAD + LLM + Partials). Returns freigegebene Bytes. */
export async function ipcDeleteAllModels(): Promise<number> {
  return invoke<number>("delete_all_models");
}

/** Löscht nur abgebrochene Downloads (`.partial`). Returns freigegebene Bytes. */
export async function ipcCleanPartialDownloads(): Promise<number> {
  return invoke<number>("clean_partial_downloads");
}

/**
 * Loescht alle Provider-API-Keys aus File-Storage **und** OS-Keychain.
 * Fehler einzelner Provider werden gesammelt — die Funktion rejected nur,
 * wenn mindestens einer fehlschlug, raeumt aber so viel wie moeglich.
 */
export async function ipcResetApiKeys(): Promise<void> {
  return invoke("reset_api_keys");
}

/**
 * Loescht die Wayland-Permission-Token-Datei. Naechster Auto-Paste-Inject
 * triggert wieder den xdg-desktop-portal-Dialog. Auf X11/Windows No-Op.
 */
export async function ipcResetWaylandToken(): Promise<void> {
  return invoke("reset_wayland_token");
}

/**
 * Vollständiger Werksreset: Provider-Keys, Wayland-Token, Modi (zurueck
 * auf die 6 Defaults) und Settings (JSON geloescht, In-Memory auf
 * Default). Heruntergeladene Modelle bleiben bewusst erhalten — siehe
 * `ipcDeleteAllModels` fuer den separaten Modell-Wipe.
 */
export async function ipcResetAppFactory(): Promise<void> {
  return invoke("reset_app_factory");
}

/**
 * Diagnose: testet den Auto-Paste-Pfad direkt, ohne die normale Pipeline
 * (kein Audio, kein STT, kein LLM). Wartet `delaySecs` Sekunden — User
 * fokussiert in der Zwischenzeit das Ziel-Fenster — und sendet dann
 * `text` per Clipboard + libei-Strg+V.
 */
export async function ipcTestAutoPaste(
  text: string,
  delaySecs: number,
): Promise<void> {
  return invoke("test_auto_paste", { text, delaySecs });
}

export async function ipcStartRecording(modeId: string): Promise<void> {
  return invoke("start_recording", { modeId });
}

export async function ipcCancelMenu(): Promise<void> {
  return invoke("cancel_menu");
}

/**
 * Effektiver Menue-Hotkey, wie er aktuell tatsaechlich gebunden ist.
 *
 * - X11/Windows: `null` — Frontend zeigt `Settings.menu_hotkey` direkt.
 * - Wayland: vom Compositor zurueckgegebener Trigger (z.B. "Meta+Space").
 *   Auf KDE darf der User in System-Settings → Globale Verknuepfungen
 *   einen anderen Hotkey zuweisen — der landet dann hier.
 */
export async function ipcGetEffectiveMenuHotkey(): Promise<string | null> {
  return invoke<string | null>("get_effective_menu_hotkey");
}

export interface ProviderStatus {
  provider: string;
  configured: boolean;
  error: string | null;
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
