// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import Field from "../components/Field";
import Button from "../components/Button";
import Banner from "../components/Banner";
import Loading from "../components/Loading";
import ApiKeysSection from "../components/ApiKeysSection";
import TestTranscriptionSection from "../components/TestTranscriptionSection";
import AutoPasteTestSection from "../components/AutoPasteTestSection";
import UpdateSection from "../components/UpdateSection";
import { useSettingsStore } from "../store";
import {
  ipcCleanPartialDownloads,
  ipcDeleteAllModels,
  ipcDeleteCachedFile,
  ipcDownloadDefaultModel,
  ipcDownloadLlmDefaultModel,
  ipcGetEffectiveMenuHotkey,
  ipcGetHardwareReport,
  ipcGetSessionInfo,
  ipcGetWhisperBackend,
  ipcListCachedFiles,
  ipcResetApiKeys,
  ipcResetAppFactory,
  ipcResetWaylandToken,
  type CachedFile,
  type HardwareReport,
  type ModelDownloadProgress,
  type SessionInfo,
  type WhisperBackendInfo,
} from "../lib/tauri";
import { recommendLlmSlot } from "../lib/recommend";
import { isWindows } from "../lib/platform";
import { emit } from "@tauri-apps/api/event";
import {
  LOCALE_NATIVE_NAMES,
  SUPPORTED_LOCALES,
  pickSupported,
  useI18nStore,
  useLocale,
  useT,
  type SupportedLocale,
  type TranslateFn,
} from "../i18n";

const inputCls =
  "bg-surface border border-outline rounded-md px-3 py-2 text-sm text-fg placeholder:text-fg-faint focus:outline-none focus:border-brand focus:ring-1 focus:ring-brand/40";

const WHISPER_SLOT_KEYS: Record<string, string> = {
  "large-v3-turbo-q8_0": "settings.whisper.slot.q8",
  "large-v3-turbo-german-q5_0": "settings.whisper.slot.de_pro",
  "large-v3-turbo-q5_0": "settings.whisper.slot.q5_light",
  "small-q5_1": "settings.whisper.slot.small",
  "large-v3-turbo": "settings.whisper.slot.f16",
};

const LLM_SLOT_KEYS: Record<string, string> = {
  "gemma4-e4b-it-q5_k_m": "settings.llm.slot.gemma4_e4b",
  "gemma4-e2b-it-q5_k_m": "settings.llm.slot.gemma4_e2b",
  "gemma3-1b-it-q5_k_m": "settings.llm.slot.gemma3_1b",
  "gemma3-4b-it-q5_k_m": "settings.llm.slot.gemma3_4b",
  "llama3.2-1b-instruct-q5_k_m": "settings.llm.slot.llama32_1b",
  "qwen2.5-1.5b-instruct-q5_k_m": "settings.llm.slot.qwen25_15b",
};

const SUBNAV_ITEMS = [
  { id: "language", key: "settings.subnav.language" },
  { id: "hardware", key: "settings.subnav.hardware" },
  { id: "audio-hotkey", key: "settings.subnav.audio_hotkey" },
  { id: "local-models", key: "settings.subnav.local_models" },
  { id: "privacy-startup", key: "settings.subnav.privacy_startup" },
  { id: "cache", key: "settings.subnav.cache" },
  { id: "diagnostics", key: "settings.subnav.diagnostics" },
  { id: "api-keys", key: "settings.subnav.api_keys" },
  { id: "danger-zone", key: "settings.subnav.danger_zone" },
] as const;

function fmtMb(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatBytes(t: TranslateFn, bytes: number): string {
  if (bytes < 1024) return t("common.unit.bytes", { value: bytes });
  if (bytes < 1024 * 1024)
    return t("common.unit.kb", { value: (bytes / 1024).toFixed(1) });
  if (bytes < 1024 * 1024 * 1024)
    return t("common.unit.mb", { value: (bytes / (1024 * 1024)).toFixed(1) });
  return t("common.unit.gb", {
    value: (bytes / (1024 * 1024 * 1024)).toFixed(2),
  });
}

export default function Settings(): JSX.Element {
  const t = useT();
  const settings = useSettingsStore((s) => s.settings);
  const loading = useSettingsStore((s) => s.loading);
  const error = useSettingsStore((s) => s.error);
  const audioDevices = useSettingsStore((s) => s.audioDevices);
  const load = useSettingsStore((s) => s.load);
  const loadAudioDevices = useSettingsStore((s) => s.loadAudioDevices);
  const update = useSettingsStore((s) => s.update);

  const [downloading, setDownloading] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const [progress, setProgress] = useState<ModelDownloadProgress | null>(null);
  const [session, setSession] = useState<SessionInfo | null>(null);
  const [effectiveHotkey, setEffectiveHotkey] = useState<string | null>(null);
  const [hardware, setHardware] = useState<HardwareReport | null>(null);
  const [activeBackend, setActiveBackend] = useState<WhisperBackendInfo | null>(
    null,
  );
  const [llmDownloading, setLlmDownloading] = useState(false);
  const [llmDownloadError, setLlmDownloadError] = useState<string | null>(null);
  const [llmProgress, setLlmProgress] = useState<ModelDownloadProgress | null>(
    null,
  );

  useEffect(() => {
    void load();
    void loadAudioDevices();
    void ipcGetSessionInfo()
      .then(setSession)
      .catch(() => null);
    void ipcGetEffectiveMenuHotkey()
      .then(setEffectiveHotkey)
      .catch(() => null);
    void ipcGetHardwareReport()
      .then(setHardware)
      .catch(() => null);
    void ipcGetWhisperBackend()
      .then(setActiveBackend)
      .catch(() => null);
  }, [load, loadAudioDevices]);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    void listen<ModelDownloadProgress>("model-download-progress", (event) =>
      setProgress(event.payload),
    ).then((fn) => unlistens.push(fn));
    void listen<ModelDownloadProgress>("llm-model-download-progress", (event) =>
      setLlmProgress(event.payload),
    ).then((fn) => unlistens.push(fn));
    return () => {
      unlistens.forEach((u) => u());
    };
  }, []);

  if (loading || !settings) {
    return <Loading label={t("settings.loading")} />;
  }

  const onPickModel = async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Whisper model (GGML)", extensions: ["bin", "gguf"] }],
    });
    if (typeof picked === "string") {
      void update({ whisper_model_path: picked });
    }
  };

  const onDownloadLlmDefault = async () => {
    setLlmDownloading(true);
    setLlmDownloadError(null);
    setLlmProgress(null);
    try {
      await ipcDownloadLlmDefaultModel();
      await load();
    } catch (e) {
      setLlmDownloadError(String(e));
    } finally {
      setLlmDownloading(false);
    }
  };

  const onDownloadDefault = async () => {
    setDownloading(true);
    setDownloadError(null);
    setProgress(null);
    try {
      const path = await ipcDownloadDefaultModel();
      void update({ whisper_model_path: path });
    } catch (e) {
      setDownloadError(String(e));
    } finally {
      setDownloading(false);
    }
  };

  const progressPct =
    progress && progress.total
      ? Math.round((progress.downloaded / progress.total) * 100)
      : null;

  const renderProgress = (p: ModelDownloadProgress): string => {
    const current = fmtMb(p.downloaded);
    if (!p.total) {
      return t("settings.whisper.progress.simple", { current });
    }
    const total = fmtMb(p.total);
    const pct = Math.round((p.downloaded / p.total) * 100);
    return t("settings.whisper.progress.with_pct", { current, total, pct });
  };

  return (
    <div className="flex gap-6 items-start">
      <SettingsSubNav />
      <div className="flex-1 flex flex-col gap-8 max-w-2xl min-w-0">
        {error ? <Banner tone="error">{error}</Banner> : null}

        <section
          id="language"
          className="scroll-mt-6 flex flex-col gap-4"
          aria-labelledby="language-heading"
        >
          <SectionHeader
            id="language-heading"
            title={t("settings.section.language")}
          />
          <LanguageField
            currentLocale={settings.locale}
            onPick={(loc) => update({ locale: loc })}
          />
        </section>

        <section
          id="hardware"
          className="scroll-mt-6 flex flex-col gap-4"
          aria-labelledby="hardware-heading"
        >
          <SectionHeader
            id="hardware-heading"
            title={t("settings.section.hardware")}
          />
          <HardwareStatusField
            hardware={hardware}
            activeBackend={activeBackend}
            currentLlmSlot={settings.llm_default_slot}
            onPickLlmSlot={(slot) => void update({ llm_default_slot: slot })}
          />
        </section>

        <section
          id="audio-hotkey"
          className="scroll-mt-6 flex flex-col gap-4"
          aria-labelledby="audio-hotkey-heading"
        >
          <SectionHeader
            id="audio-hotkey-heading"
            title={t("settings.section.audio_hotkey")}
          />
          <Field
            label={t("settings.audio.label")}
            hint={t("settings.audio.hint")}
          >
            <select
              className={inputCls}
              value={settings.audio_input_device ?? ""}
              onChange={(e) =>
                void update({
                  audio_input_device: e.target.value || null,
                })
              }
            >
              <option value="">{t("settings.audio.os_default")}</option>
              {audioDevices.map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
          </Field>

          <MenuHotkeyField
            session={session}
            settingsValue={settings.menu_hotkey}
            effective={effectiveHotkey}
            onChange={(v) => void update({ menu_hotkey: v })}
          />
        </section>

        <section
          id="local-models"
          className="scroll-mt-6 flex flex-col gap-4"
          aria-labelledby="local-models-heading"
        >
          <SectionHeader
            id="local-models-heading"
            title={t("settings.section.local_models")}
          />
          <Field
            label={t("settings.whisper.label")}
            hint={t("settings.whisper.hint")}
          >
            <div className="flex gap-2">
              <input
                type="text"
                className={`${inputCls} flex-1`}
                placeholder={t("settings.whisper.path_placeholder")}
                value={settings.whisper_model_path ?? ""}
                onChange={(e) =>
                  void update({
                    whisper_model_path: e.target.value || null,
                  })
                }
              />
              <Button variant="secondary" onClick={() => void onPickModel()}>
                {t("settings.whisper.pick_file")}
              </Button>
            </div>
            <select
              className={inputCls}
              value={settings.whisper_default_slot}
              onChange={(e) =>
                void update({ whisper_default_slot: e.target.value })
              }
            >
              {Object.entries(WHISPER_SLOT_KEYS).map(([slot, key]) => (
                <option key={slot} value={slot}>
                  {t(key)}
                </option>
              ))}
            </select>
            <div className="flex flex-col gap-1.5">
              <Button
                onClick={() => void onDownloadDefault()}
                disabled={downloading}
                className="self-start"
              >
                {downloading
                  ? t("settings.whisper.btn.busy")
                  : t("settings.whisper.btn.idle")}
              </Button>
              {progress ? (
                <div className="flex flex-col gap-1 text-xs text-fg-muted">
                  <div>{renderProgress(progress)}</div>
                  {progressPct !== null ? (
                    <div className="h-1.5 bg-elevated rounded-full overflow-hidden">
                      <div
                        className="h-full bg-brand transition-all"
                        style={{ width: `${progressPct}%` }}
                      />
                    </div>
                  ) : null}
                </div>
              ) : null}
              {downloadError ? (
                <div className="text-xs text-status-error">{downloadError}</div>
              ) : null}
            </div>
          </Field>

          <Field
            label={t("settings.whisper_threads.label")}
            hint={t("settings.whisper_threads.hint")}
          >
            <input
              type="number"
              min={1}
              max={32}
              className={`${inputCls} w-32`}
              placeholder={t("settings.whisper_threads.placeholder")}
              value={settings.whisper_n_threads ?? ""}
              onChange={(e) => {
                const v = e.target.value.trim();
                if (v === "") {
                  void update({ whisper_n_threads: null });
                } else {
                  const n = parseInt(v, 10);
                  if (!Number.isNaN(n) && n >= 1 && n <= 32) {
                    void update({ whisper_n_threads: n });
                  }
                }
              }}
            />
          </Field>

          <Field
            label={t("settings.beam_size.label")}
            hint={t("settings.beam_size.hint")}
          >
            <input
              type="number"
              min={1}
              max={10}
              className={`${inputCls} w-32`}
              value={settings.whisper_beam_size}
              onChange={(e) => {
                const n = parseInt(e.target.value, 10);
                if (!Number.isNaN(n) && n >= 1 && n <= 10) {
                  void update({ whisper_beam_size: n });
                }
              }}
            />
          </Field>

          {/* Embedded LLM (llama-cpp-2) is Linux/macOS-only — on Windows it
              collides with whisper's ggml at link time (issue #1). There we
              hide the GGUF slot picker + download and point the user at
              Ollama (configured below) or a cloud provider. */}
          {isWindows() ? (
            <Field
              label={t("settings.llm.label")}
              hint={t("settings.windows.local_llm.hint")}
            >
              <p className="text-sm text-fg-muted">
                {t("settings.windows.local_llm.body")}
              </p>
            </Field>
          ) : (
            <Field
              label={t("settings.llm.label")}
              hint={t("settings.llm.hint")}
            >
              <select
                className={inputCls}
                value={settings.llm_default_slot}
                onChange={(e) =>
                  void update({ llm_default_slot: e.target.value })
                }
              >
                {Object.entries(LLM_SLOT_KEYS).map(([slot, key]) => (
                  <option key={slot} value={slot}>
                    {t(key)}
                  </option>
                ))}
              </select>
              <input
                type="text"
                className={inputCls}
                placeholder={t("settings.llm.path_placeholder")}
                value={settings.llm_model_path ?? ""}
                onChange={(e) =>
                  void update({ llm_model_path: e.target.value || null })
                }
              />
              <div className="flex flex-col gap-1.5">
                <Button
                  onClick={() => void onDownloadLlmDefault()}
                  disabled={llmDownloading}
                  className="self-start"
                >
                  {llmDownloading
                    ? t("settings.llm.btn.busy")
                    : t("settings.llm.btn.idle")}
                </Button>
                {llmProgress ? (
                  <div className="flex flex-col gap-1 text-xs text-fg-muted">
                    <div>{renderProgress(llmProgress)}</div>
                    {llmProgress.total ? (
                      <div className="h-1.5 bg-elevated rounded-full overflow-hidden">
                        <div
                          className="h-full bg-brand transition-all"
                          style={{
                            width: `${Math.round((llmProgress.downloaded / llmProgress.total) * 100)}%`,
                          }}
                        />
                      </div>
                    ) : null}
                  </div>
                ) : null}
                {llmDownloadError ? (
                  <div className="text-xs text-status-error">
                    {llmDownloadError}
                  </div>
                ) : null}
              </div>
            </Field>
          )}

          <OllamaAdvancedSection
            url={settings.ollama_url}
            keepAlive={settings.ollama_keep_alive}
            onUrlChange={(v) => void update({ ollama_url: v })}
            onKeepAliveChange={(v) => void update({ ollama_keep_alive: v })}
          />
        </section>

        <section
          id="privacy-startup"
          className="scroll-mt-6 flex flex-col gap-4"
          aria-labelledby="privacy-startup-heading"
        >
          <SectionHeader
            id="privacy-startup-heading"
            title={t("settings.section.privacy_startup")}
          />
          <Field
            label={t("settings.autostart.label")}
            hint={t("settings.autostart.hint")}
          >
            <label className="flex items-center gap-2 text-sm text-fg">
              <input
                type="checkbox"
                checked={settings.autostart}
                onChange={(e) => void update({ autostart: e.target.checked })}
              />
              {t("common.enable")}
            </label>
          </Field>
        </section>

        <section
          id="cache"
          className="scroll-mt-6 flex flex-col gap-4"
          aria-labelledby="cache-heading"
        >
          <SectionHeader
            id="cache-heading"
            title={t("settings.section.cache")}
          />
          <CacheManagementField />
        </section>

        <section
          id="diagnostics"
          className="scroll-mt-6 flex flex-col gap-4"
          aria-labelledby="diagnostics-heading"
        >
          <SectionHeader
            id="diagnostics-heading"
            title={t("settings.section.diagnostics")}
          />
          <UpdateSection />
          <SetupAssistantRetriggerField
            onTrigger={() => void update({ onboarding_done: false })}
          />
          <TestTranscriptionSection />
          <AutoPasteTestSection />
        </section>

        <section
          id="api-keys"
          className="scroll-mt-6"
          aria-labelledby="api-keys-heading"
        >
          <ApiKeysSection />
        </section>

        <section
          id="danger-zone"
          className="scroll-mt-6"
          aria-labelledby="danger-zone-heading"
        >
          <DangerZoneSection />
        </section>
      </div>
    </div>
  );
}

/**
 * Sticky sub-nav for the Settings page. Hidden on small windows
 * (<1024 px) — the user then scrolls linearly through.
 */
function SettingsSubNav(): JSX.Element {
  const t = useT();
  return (
    <nav
      aria-label={t("settings.subnav.aria")}
      className="hidden lg:flex flex-col gap-0.5 w-44 shrink-0 sticky top-0 self-start"
    >
      {SUBNAV_ITEMS.map((it) => (
        <a
          key={it.id}
          href={`#${it.id}`}
          className="px-3 py-1.5 rounded-md text-sm text-fg-muted hover:text-fg hover:bg-elevated transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40"
        >
          {t(it.key)}
        </a>
      ))}
    </nav>
  );
}

interface SectionHeaderProps {
  id: string;
  title: string;
}

function SectionHeader({ id, title }: SectionHeaderProps): JSX.Element {
  return (
    <h2
      id={id}
      className="text-lg font-semibold text-fg border-b border-outline pb-2"
    >
      {title}
    </h2>
  );
}

/**
 * Language switcher. Reads the currently *active* (resolved) locale
 * from useLocale, writes the *raw* locale (= concrete SupportedLocale
 * code) back into settings, and emits a Tauri event so the other
 * webview windows (overlay, menu) can update their useI18nStore.
 */
function LanguageField({
  currentLocale,
  onPick,
}: {
  currentLocale: string | null;
  onPick: (locale: string) => Promise<void>;
}): JSX.Element {
  const t = useT();
  const activeLocale = useLocale();
  const resolved = pickSupported(currentLocale);
  return (
    <Field
      label={t("settings.language.label")}
      hint={t("settings.language.hint")}
    >
      <select
        className={`${inputCls} w-72`}
        value={resolved}
        onChange={(e) => {
          const next = e.target.value as SupportedLocale;
          // Persist FIRST, then update the UI and broadcast the event.
          // If ipcSetSettings fails we avoid the inconsistent state
          // "UI is on the new language but settings are not persisted".
          // The settings store sets `error` on failure, which becomes
          // visible via the banner.
          void onPick(next).then(() => {
            useI18nStore.setState({ locale: next });
            // Cross-window sync: overlay + menu listen for this event
            // in main.tsx and update their store accordingly.
            void emit("i18n://locale-changed", { locale: next });
          });
        }}
      >
        {SUPPORTED_LOCALES.map((loc) => (
          <option key={loc} value={loc}>
            {LOCALE_NATIVE_NAMES[loc]}
          </option>
        ))}
      </select>
      {activeLocale !== resolved ? (
        // Defensive — should never trigger after a successful persist.
        // For the rare race (multiple switchers used simultaneously in
        // parallel-opened settings windows) it at least surfaces the
        // drift.
        <div className="text-xs text-status-error">
          UI: {activeLocale} ≠ Settings: {resolved}
        </div>
      ) : null}
    </Field>
  );
}

function OllamaAdvancedSection({
  url,
  keepAlive,
  onUrlChange,
  onKeepAliveChange,
}: {
  url: string;
  keepAlive: string;
  onUrlChange: (v: string) => void;
  onKeepAliveChange: (v: string) => void;
}): JSX.Element {
  const t = useT();
  return (
    <details className="rounded-md border border-outline/60 bg-surface/40 p-3">
      <summary className="text-sm font-medium text-fg-muted cursor-pointer hover:text-fg transition-colors select-none">
        {t("settings.ollama.summary")}
      </summary>
      <div className="flex flex-col gap-4 pt-3 mt-2 border-t border-outline/40">
        <p className="text-xs text-fg-faint">
          {t("settings.ollama.intro_prefix")}{" "}
          <code className="text-fg-muted font-mono">
            local_engine = &quot;ollama&quot;
          </code>{" "}
          {t("settings.ollama.intro_suffix")}
        </p>
        <Field
          label={t("settings.ollama.endpoint.label")}
          hint={t("settings.ollama.endpoint.hint")}
        >
          <input
            type="text"
            className={inputCls}
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            placeholder="http://127.0.0.1:11434"
          />
        </Field>
        <Field
          label={t("settings.ollama.keep_alive.label")}
          hint={t("settings.ollama.keep_alive.hint")}
        >
          <input
            type="text"
            className={`${inputCls} w-32`}
            value={keepAlive}
            onChange={(e) => onKeepAliveChange(e.target.value)}
            placeholder="5m"
          />
        </Field>
      </div>
    </details>
  );
}

function SetupAssistantRetriggerField({
  onTrigger,
}: {
  onTrigger: () => void;
}): JSX.Element {
  const t = useT();
  const handler = () => {
    if (window.confirm(t("settings.setup_assistant.confirm"))) {
      onTrigger();
    }
  };
  return (
    <Field
      label={t("settings.setup_assistant.label")}
      hint={t("settings.setup_assistant.hint")}
    >
      <Button variant="secondary" onClick={handler} className="self-start">
        {t("settings.setup_assistant.btn")}
      </Button>
    </Field>
  );
}

function MenuHotkeyField({
  session,
  settingsValue,
  effective,
  onChange,
}: {
  session: SessionInfo | null;
  settingsValue: string;
  effective: string | null;
  onChange: (v: string) => void;
}): JSX.Element {
  const t = useT();
  const isWayland = session?.display_server === "wayland";

  if (isWayland) {
    return (
      <Field
        label={t("settings.hotkey.wayland.label")}
        hint={t("settings.hotkey.wayland.hint")}
      >
        <div className="bg-elevated border border-outline rounded-md px-3 py-2 text-sm font-mono w-72 text-fg-muted">
          {effective ?? settingsValue}
        </div>
      </Field>
    );
  }

  return (
    <Field label={t("settings.hotkey.label")} hint={t("settings.hotkey.hint")}>
      <input
        type="text"
        className={`${inputCls} font-mono w-72`}
        value={settingsValue}
        onChange={(e) => onChange(e.target.value)}
        placeholder="CommandOrControl+Alt+Space"
      />
    </Field>
  );
}

function HardwareStatusField({
  hardware,
  activeBackend,
  currentLlmSlot,
  onPickLlmSlot,
}: {
  hardware: HardwareReport | null;
  activeBackend: WhisperBackendInfo | null;
  currentLlmSlot: string;
  onPickLlmSlot: (slot: string) => void;
}): JSX.Element {
  const t = useT();

  if (!hardware && !activeBackend) {
    return (
      <Field
        label={t("settings.hardware.detecting_label")}
        hint={t("settings.hardware.detecting_hint")}
      >
        <Loading label={t("settings.hardware.loading")} inline />
      </Field>
    );
  }

  const ramText =
    hardware && hardware.total_ram_gb > 0
      ? t("settings.hardware.ram_value", {
          total: hardware.total_ram_gb.toFixed(1),
          available: hardware.available_ram_gb.toFixed(1),
        })
      : "—";

  const gpuFlags: string[] = [];
  if (hardware?.has_nvidia_gpu) gpuFlags.push("NVIDIA");
  if (hardware?.has_amd_gpu) gpuFlags.push("AMD");
  if (hardware?.is_apple_silicon) gpuFlags.push("Apple Silicon");
  if (gpuFlags.length === 0) gpuFlags.push(t("settings.hardware.no_gpu"));

  const libsFlags: string[] = [];
  if (hardware?.has_vulkan) libsFlags.push("Vulkan");
  if (hardware?.has_openblas) libsFlags.push("OpenBLAS");
  if (libsFlags.length === 0) libsFlags.push(t("settings.hardware.no_libs"));

  const showVariantHint =
    hardware &&
    activeBackend &&
    hardware.recommended_variant !== activeBackend.backend &&
    hardware.recommended_speedup > activeBackend.expected_speedup * 1.2;

  const llmRecommendation = hardware
    ? recommendLlmSlot(hardware.total_ram_gb)
    : null;
  const llmMismatch =
    llmRecommendation !== null && llmRecommendation.slot !== currentLlmSlot;

  return (
    <Field
      label={t("settings.hardware.label")}
      hint={t("settings.hardware.hint")}
    >
      <div className="flex flex-col gap-1.5 text-sm text-fg-muted">
        <div>
          <span className="text-fg-faint">
            {t("settings.hardware.active_backend")}
          </span>{" "}
          <span className="font-medium text-fg">
            {activeBackend?.backend ?? "—"}
          </span>
          {activeBackend ? (
            <span className="text-fg-faint">
              {" "}
              {`(${t("settings.hardware.backend_desc", {
                description: activeBackend.description,
                factor: activeBackend.expected_speedup,
              })})`}
            </span>
          ) : null}
        </div>
        <div>
          <span className="text-fg-faint">
            {t("settings.hardware.cpu_threads")}
          </span>{" "}
          {hardware?.cpu_logical_cores ?? "—"}
        </div>
        <div>
          <span className="text-fg-faint">{t("settings.hardware.ram")}</span>{" "}
          {ramText}
        </div>
        <div>
          <span className="text-fg-faint">{t("settings.hardware.gpu")}</span>{" "}
          {gpuFlags.join(", ")}
        </div>
        <div>
          <span className="text-fg-faint">
            {t("settings.hardware.compute_libs")}
          </span>{" "}
          {libsFlags.join(", ")}
        </div>
        {llmRecommendation && !isWindows() ? (
          <div>
            <span className="text-fg-faint">
              {t("settings.hardware.llm_recommendation")}
            </span>{" "}
            <span className="font-medium text-fg">
              {llmRecommendation.label}
            </span>
            {llmMismatch ? (
              <button
                type="button"
                onClick={() => onPickLlmSlot(llmRecommendation.slot)}
                className="ml-2 text-xs underline text-brand hover:text-brand-hover pointer-events-auto"
              >
                {t("settings.hardware.adopt")}
              </button>
            ) : (
              <span className="ml-2 text-xs text-status-recording">
                {t("settings.hardware.active_check")}
              </span>
            )}
          </div>
        ) : null}
        {showVariantHint ? (
          <div className="mt-1 text-xs rounded-md bg-brand/10 border border-brand/40 px-2.5 py-1.5 text-fg">
            {t("settings.hardware.variant_hint", {
              variant: hardware.recommended_variant,
              recommended: hardware.recommended_speedup,
              current: activeBackend.expected_speedup,
            })}
          </div>
        ) : null}
      </div>
    </Field>
  );
}

function CacheManagementField(): JSX.Element {
  const t = useT();
  const [files, setFiles] = useState<CachedFile[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const list = await ipcListCachedFiles();
      setFiles(list);
    } catch (e) {
      setStatus(t("settings.cache.status.list_failed", { message: String(e) }));
    }
  };

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const kindLabel = (kind: CachedFile["kind"]): string => {
    switch (kind) {
      case "whisper":
        return t("settings.cache.kind.whisper");
      case "vad":
        return t("settings.cache.kind.vad");
      case "llm":
        return t("settings.cache.kind.llm");
      case "partial":
        return t("settings.cache.kind.partial");
      case "other":
        return t("settings.cache.kind.other");
    }
  };

  const kindColor = (kind: CachedFile["kind"]): string => {
    switch (kind) {
      case "whisper":
        return "bg-brand/15 text-brand";
      case "vad":
        return "bg-status-processing/15 text-status-processing";
      case "llm":
        return "bg-status-done/15 text-status-done";
      case "partial":
        return "bg-status-error/15 text-status-error";
      case "other":
        return "bg-elevated text-fg-muted";
    }
  };

  const onDeleteSingle = async (filename: string) => {
    if (!confirm(t("settings.cache.confirm_single", { filename }))) return;
    setBusy(true);
    setStatus(null);
    try {
      const freed = await ipcDeleteCachedFile(filename);
      setStatus(
        t("settings.cache.status.deleted_single", {
          size: formatBytes(t, freed),
        }),
      );
      await refresh();
    } catch (e) {
      setStatus(
        t("settings.cache.status.delete_failed", { message: String(e) }),
      );
    } finally {
      setBusy(false);
    }
  };

  const onCleanPartials = async () => {
    setBusy(true);
    setStatus(null);
    try {
      const freed = await ipcCleanPartialDownloads();
      setStatus(
        t("settings.cache.status.partials_cleaned", {
          size: formatBytes(t, freed),
        }),
      );
      await refresh();
    } catch (e) {
      setStatus(
        t("settings.cache.status.cleanup_failed", { message: String(e) }),
      );
    } finally {
      setBusy(false);
    }
  };

  const onDeleteAll = async () => {
    if (!confirm(t("settings.cache.confirm_all"))) {
      return;
    }
    setBusy(true);
    setStatus(null);
    try {
      const freed = await ipcDeleteAllModels();
      setStatus(
        t("settings.cache.status.deleted_all", {
          size: formatBytes(t, freed),
        }),
      );
      await refresh();
    } catch (e) {
      setStatus(
        t("settings.cache.status.delete_failed", { message: String(e) }),
      );
    } finally {
      setBusy(false);
    }
  };

  const totalBytes = files?.reduce((sum, f) => sum + f.size_bytes, 0) ?? 0;

  return (
    <Field label={t("settings.cache.label")} hint={t("settings.cache.hint")}>
      <div className="flex flex-col gap-3">
        {files === null ? (
          <Loading label={t("settings.cache.loading")} inline />
        ) : files.length === 0 ? (
          <div className="text-sm text-fg-faint">
            {t("settings.cache.empty")}
          </div>
        ) : (
          <div className="flex flex-col gap-1.5">
            {files.map((f) => (
              <div
                key={f.filename}
                className="flex items-center gap-2 text-sm py-1.5 px-2 rounded-md border border-outline bg-surface"
              >
                <span
                  className={`shrink-0 inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${kindColor(f.kind)}`}
                >
                  {kindLabel(f.kind)}
                </span>
                <span className="flex-1 font-mono text-xs text-fg-muted truncate">
                  {f.filename}
                </span>
                <span className="shrink-0 text-xs text-fg-faint w-20 text-right">
                  {formatBytes(t, f.size_bytes)}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void onDeleteSingle(f.filename)}
                  disabled={busy}
                >
                  {t("settings.cache.delete_button")}
                </Button>
              </div>
            ))}
            <div className="text-xs text-fg-faint pt-1">
              {t("settings.cache.total", {
                count: files.length,
                size: formatBytes(t, totalBytes),
              })}
            </div>
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            onClick={() => void onCleanPartials()}
            disabled={busy}
          >
            {t("settings.cache.clean_partials")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => void onDeleteAll()}
            disabled={busy}
          >
            {t("settings.cache.delete_all")}
          </Button>
          <Button
            variant="ghost"
            onClick={() => void refresh()}
            disabled={busy}
          >
            {t("common.refresh")}
          </Button>
        </div>

        {status ? <div className="text-xs text-fg-muted">{status}</div> : null}
      </div>
    </Field>
  );
}

function DangerZoneSection(): JSX.Element {
  const t = useT();
  const [busy, setBusy] = useState<string | null>(null);
  const [status, setStatus] = useState<{
    kind: "ok" | "err";
    text: string;
  } | null>(null);

  const run = async (
    label: string,
    confirmMsg: string,
    action: () => Promise<void>,
    successMsg: string,
  ) => {
    if (!confirm(confirmMsg)) return;
    setBusy(label);
    setStatus(null);
    try {
      await action();
      setStatus({ kind: "ok", text: successMsg });
    } catch (e) {
      setStatus({ kind: "err", text: String(e) });
    } finally {
      setBusy(null);
    }
  };

  const onResetApiKeys = () =>
    run(
      "keys",
      t("settings.danger.api_keys.confirm"),
      ipcResetApiKeys,
      t("settings.danger.api_keys.success"),
    );

  const onResetWayland = () =>
    run(
      "wayland",
      t("settings.danger.wayland.confirm"),
      ipcResetWaylandToken,
      t("settings.danger.wayland.success"),
    );

  const onFactoryReset = () =>
    run(
      "factory",
      t("settings.danger.factory.confirm"),
      ipcResetAppFactory,
      t("settings.danger.factory.success"),
    );

  return (
    <Field label={t("settings.danger.label")} hint={t("settings.danger.hint")}>
      <div className="rounded-md border border-status-error/40 bg-status-error/5 p-3 flex flex-col gap-3">
        <DangerRow
          title={t("settings.danger.api_keys.title")}
          description={t("settings.danger.api_keys.description")}
          buttonLabel={
            busy === "keys"
              ? t("settings.danger.api_keys.busy")
              : t("settings.danger.api_keys.button")
          }
          onClick={() => void onResetApiKeys()}
          disabled={busy !== null}
        />
        <DangerRow
          title={t("settings.danger.wayland.title")}
          description={t("settings.danger.wayland.description")}
          buttonLabel={
            busy === "wayland"
              ? t("settings.danger.wayland.busy")
              : t("settings.danger.wayland.button")
          }
          onClick={() => void onResetWayland()}
          disabled={busy !== null}
        />
        <DangerRow
          title={t("settings.danger.factory.title")}
          description={t("settings.danger.factory.description")}
          buttonLabel={
            busy === "factory"
              ? t("settings.danger.factory.busy")
              : t("settings.danger.factory.button")
          }
          onClick={() => void onFactoryReset()}
          disabled={busy !== null}
          severe
        />
        {status ? (
          <div
            className={`text-xs ${
              status.kind === "ok" ? "text-status-done" : "text-status-error"
            }`}
          >
            {status.text}
          </div>
        ) : null}
        <div className="text-xs text-fg-faint">
          {t("settings.danger.uninstall_hint_prefix")}{" "}
          <code className="text-fg-muted">scripts/uninstall-cleanup.sh</code>{" "}
          {t("settings.danger.uninstall_hint_suffix")}
        </div>
      </div>
    </Field>
  );
}

interface DangerRowProps {
  title: string;
  description: string;
  buttonLabel: string;
  onClick: () => void;
  disabled?: boolean;
  severe?: boolean;
}

function DangerRow({
  title,
  description,
  buttonLabel,
  onClick,
  disabled,
  severe,
}: DangerRowProps): JSX.Element {
  return (
    <div className="flex items-center justify-between gap-3">
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium text-fg">{title}</div>
        <div className="text-xs text-fg-muted">{description}</div>
      </div>
      <Button
        size="sm"
        variant={severe ? "danger-strong" : "secondary"}
        onClick={onClick}
        disabled={disabled}
      >
        {buttonLabel}
      </Button>
    </div>
  );
}
