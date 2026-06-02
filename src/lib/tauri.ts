// SPDX-License-Identifier: GPL-3.0-or-later
// Thin wrapper around Tauri's invoke(), with error normalization and
// IPC command names centralized in one place.

import { invoke } from "@tauri-apps/api/core";
import type { Mode, Settings } from "./types";

/**
 * Tauri returns this exact error when an IPC command tries to resolve its
 * `tauri::State` before `app.manage()` has registered it. On first launch the
 * webview can `invoke` `get_settings`/`get_modes` before the setup hook
 * finishes its one-time disk work — a transient race (issue #7).
 */
export function isUnmanagedStateError(e: unknown): boolean {
  return String(e).includes("State not managed");
}

/**
 * Run `fn`, retrying with a short fixed backoff *only* while it fails with the
 * unmanaged-state error (the first-launch setup race, #7). Any other error
 * propagates immediately, and a still-unmanaged state after the attempt budget
 * surfaces the original error — so a genuine failure is never masked.
 */
export async function retryWhileUnmanaged<T>(
  fn: () => Promise<T>,
  attempts = 10,
  delayMs = 100,
): Promise<T> {
  for (let attempt = 1; ; attempt++) {
    try {
      return await fn();
    } catch (e) {
      if (attempt >= attempts || !isUnmanagedStateError(e)) throw e;
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
  }
}

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
  /** Total RAM in GB. 0 = detection not implemented on this OS. */
  total_ram_gb: number;
  /** Currently available RAM in GB. 0 = not implemented. */
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
 * Downloads the GGUF LLM model selected in `Settings.llm_default_slot`
 * to `app_config_dir/models/`. Progress arrives as
 * `llm-model-download-progress` events (separate from Whisper
 * progress).
 *
 * Returns: absolute path to the downloaded GGUF file.
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

/** Lists all files in the `app_config_dir/models/` cache. */
export async function ipcListCachedFiles(): Promise<CachedFile[]> {
  return invoke<CachedFile[]>("list_cached_files");
}

/** Deletes a single file. Returns the freed bytes. */
export async function ipcDeleteCachedFile(filename: string): Promise<number> {
  return invoke<number>("delete_cached_file", { filename });
}

/** Deletes all model files (Whisper + VAD + LLM + partials). Returns the freed bytes. */
export async function ipcDeleteAllModels(): Promise<number> {
  return invoke<number>("delete_all_models");
}

/** Deletes only aborted downloads (`.partial`). Returns the freed bytes. */
export async function ipcCleanPartialDownloads(): Promise<number> {
  return invoke<number>("clean_partial_downloads");
}

/**
 * Deletes all provider API keys from file storage **and** the OS
 * keychain. Errors from individual providers are collected — the
 * function only rejects when at least one failed, but cleans up as much
 * as possible.
 */
export async function ipcResetApiKeys(): Promise<void> {
  return invoke("reset_api_keys");
}

/**
 * Deletes the Wayland permission token file. The next auto-paste
 * inject triggers the xdg-desktop-portal dialog again. No-op on
 * X11/Windows.
 */
export async function ipcResetWaylandToken(): Promise<void> {
  return invoke("reset_wayland_token");
}

/**
 * Full factory reset: provider keys, Wayland token, modes (back to the
 * 6 defaults) and settings (JSON deleted, in-memory reset to default).
 * Downloaded models are intentionally preserved — see
 * `ipcDeleteAllModels` for the separate model wipe.
 */
export async function ipcResetAppFactory(): Promise<void> {
  return invoke("reset_app_factory");
}

/**
 * Diagnostic: tests the auto-paste path directly, without the normal
 * pipeline (no audio, no STT, no LLM). Waits `delaySecs` seconds —
 * the user focuses the target window in the meantime — and then
 * sends `text` via clipboard + libei-Ctrl+V.
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
 * Effective menu hotkey, as it is actually bound right now.
 *
 * - X11/Windows: `null` — the frontend shows `Settings.menu_hotkey`
 *   directly.
 * - Wayland: the trigger returned by the compositor (e.g.
 *   "Meta+Space"). On KDE the user may assign a different hotkey via
 *   System Settings → Global Shortcuts — which then lands here.
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

export async function ipcIsSecretsEncryptedAtRest(): Promise<boolean> {
  return invoke<boolean>("is_secrets_encrypted_at_rest");
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
