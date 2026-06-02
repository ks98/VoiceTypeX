// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useRef, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  ipcDownloadDefaultModel,
  ipcDownloadLlmDefaultModel,
  ipcGetHardwareReport,
  ipcGetWhisperBackend,
  ipcSetProviderKey,
  ipcTestProviderConnection,
  type HardwareReport,
  type ModelDownloadProgress,
  type WhisperBackendInfo,
} from "../lib/tauri";
import { recommendLlmSlot } from "../lib/recommend";
import { isWindows } from "../lib/platform";
import { useSettingsStore } from "../store";
import Banner from "./Banner";
import Button from "./Button";
import Input from "./Input";
import Logo from "./Logo";
import {
  LOCALE_NATIVE_NAMES,
  SUPPORTED_LOCALES,
  useI18nStore,
  useLocale,
  useT,
  type SupportedLocale,
  type TranslateFn,
} from "../i18n";

type Step = 1 | 2 | 3 | 4 | 5;
const TOTAL_STEPS = 5;

interface OnboardingWizardProps {
  onClose: () => void;
}

/**
 * Visibility of a completed-download check mark in milliseconds,
 * before the mini progress bar is hidden entirely.
 */
const DONE_FLASH_MS = 2000;

type DownloadStatus =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "done" }
  | { kind: "error"; msg: string };

export default function OnboardingWizard({
  onClose,
}: OnboardingWizardProps): JSX.Element {
  const t = useT();
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);
  const locale = useLocale();

  // First-run language picker (issue #6): the OS-detected locale can be
  // wrong — on Windows `sys-locale` reports the display/UI language, not the
  // regional format — so the user confirms/changes it right here. Persist
  // first, then switch this window's UI and broadcast to the overlay/menu
  // windows (identical flow to the Settings language switcher).
  const onPickLocale = (next: SupportedLocale) => {
    void update({ locale: next }).then(() => {
      useI18nStore.setState({ locale: next });
      void emit("i18n://locale-changed", { locale: next });
    });
  };

  const [step, setStep] = useState<Step>(1);

  // Whisper download — background, persistent across step changes.
  const [whisperStatus, setWhisperStatus] = useState<DownloadStatus>({
    kind: "idle",
  });
  const [whisperProgress, setWhisperProgress] =
    useState<ModelDownloadProgress | null>(null);

  // LLM download — analogous.
  const [llmStatus, setLlmStatus] = useState<DownloadStatus>({ kind: "idle" });
  const [llmProgress, setLlmProgress] = useState<ModelDownloadProgress | null>(
    null,
  );

  // Auto-hide done flashes after DONE_FLASH_MS. Refs let a new download
  // cancel the previous timer.
  const [whisperFlashVisible, setWhisperFlashVisible] = useState(false);
  const [llmFlashVisible, setLlmFlashVisible] = useState(false);
  const whisperFlashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const llmFlashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [xaiKey, setXaiKey] = useState("");
  const [keyStatus, setKeyStatus] = useState<
    null | { kind: "saving" } | { kind: "ok" } | { kind: "error"; msg: string }
  >(null);

  const [backend, setBackend] = useState<WhisperBackendInfo | null>(null);
  const [hardware, setHardware] = useState<HardwareReport | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    void listen<ModelDownloadProgress>("model-download-progress", (event) =>
      setWhisperProgress(event.payload),
    ).then((fn) => unlistens.push(fn));
    void listen<ModelDownloadProgress>("llm-model-download-progress", (event) =>
      setLlmProgress(event.payload),
    ).then((fn) => unlistens.push(fn));
    void ipcGetWhisperBackend()
      .then(setBackend)
      .catch(() => null);
    void ipcGetHardwareReport()
      .then(setHardware)
      .catch(() => null);
    return () => {
      unlistens.forEach((u) => u());
      if (whisperFlashTimer.current) clearTimeout(whisperFlashTimer.current);
      if (llmFlashTimer.current) clearTimeout(llmFlashTimer.current);
    };
  }, []);

  // Whisper download starts in the background; the wizard jumps straight
  // to the next step. Progress events populate the sticky mini bar in
  // the header.
  const onDownload = () => {
    setWhisperStatus({ kind: "running" });
    setWhisperProgress(null);
    setWhisperFlashVisible(false);
    if (whisperFlashTimer.current) {
      clearTimeout(whisperFlashTimer.current);
      whisperFlashTimer.current = null;
    }
    setStep(3);
    void (async () => {
      try {
        const path = await ipcDownloadDefaultModel();
        await update({ whisper_model_path: path });
        setWhisperStatus({ kind: "done" });
        setWhisperFlashVisible(true);
        whisperFlashTimer.current = setTimeout(() => {
          setWhisperFlashVisible(false);
        }, DONE_FLASH_MS);
      } catch (e) {
        setWhisperStatus({ kind: "error", msg: String(e) });
      }
    })();
  };

  const onPickLlmSlot = async (slot: string) => {
    await update({ llm_default_slot: slot });
  };

  const onLlmDownload = () => {
    setLlmStatus({ kind: "running" });
    setLlmProgress(null);
    setLlmFlashVisible(false);
    if (llmFlashTimer.current) {
      clearTimeout(llmFlashTimer.current);
      llmFlashTimer.current = null;
    }
    setStep(5);
    void (async () => {
      try {
        await ipcDownloadLlmDefaultModel();
        setLlmStatus({ kind: "done" });
        setLlmFlashVisible(true);
        llmFlashTimer.current = setTimeout(() => {
          setLlmFlashVisible(false);
        }, DONE_FLASH_MS);
      } catch (e) {
        setLlmStatus({ kind: "error", msg: String(e) });
      }
    })();
  };

  const onSaveKey = async () => {
    setKeyStatus({ kind: "saving" });
    try {
      await ipcSetProviderKey("xai", xaiKey);
      try {
        await ipcTestProviderConnection("xai");
        setKeyStatus({ kind: "ok" });
      } catch (e) {
        setKeyStatus({
          kind: "error",
          msg: t("common.error_prefix", { message: String(e) }),
        });
      }
    } catch (e) {
      setKeyStatus({ kind: "error", msg: String(e) });
    }
  };

  const onFinish = async () => {
    await update({ onboarding_done: true });
    onClose();
  };

  const skipAll = async () => {
    await update({ onboarding_done: true });
    onClose();
  };

  const anyRunning =
    whisperStatus.kind === "running" || llmStatus.kind === "running";

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-surface border border-outline rounded-xl max-w-2xl w-full overflow-auto shadow-2xl">
        <div className="px-6 pt-6 pb-4 border-b border-outline">
          <div className="flex justify-between items-start mb-4">
            <div>
              <h2 className="text-xl font-semibold text-fg tracking-tight">
                {t("wizard.header.title")}
              </h2>
              <div className="text-xs text-fg-faint mt-0.5">
                {t("wizard.header.subtitle", { total: TOTAL_STEPS })}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <select
                aria-label={t("wizard.header.language_aria")}
                value={locale}
                onChange={(e) =>
                  onPickLocale(e.target.value as SupportedLocale)
                }
                className="bg-elevated border border-outline rounded-md text-xs text-fg-muted px-2 py-1 focus:outline-none focus:ring-1 focus:ring-brand"
              >
                {SUPPORTED_LOCALES.map((l) => (
                  <option key={l} value={l}>
                    {LOCALE_NATIVE_NAMES[l]}
                  </option>
                ))}
              </select>
              <Button variant="ghost" size="sm" onClick={() => void skipAll()}>
                {t("wizard.header.skip")}
              </Button>
            </div>
          </div>
          <StepIndicator current={step} total={TOTAL_STEPS} />
          <MiniProgressStack
            whisperStatus={whisperStatus}
            whisperProgress={whisperProgress}
            whisperFlashVisible={whisperFlashVisible}
            llmStatus={llmStatus}
            llmProgress={llmProgress}
            llmFlashVisible={llmFlashVisible}
          />
        </div>

        <div className="px-6 py-8 min-h-[360px]">
          {step === 1 ? (
            <StepWelcome />
          ) : step === 2 ? (
            <StepDownload
              onDownload={onDownload}
              downloadStarted={
                whisperStatus.kind !== "idle" || settings?.whisper_model_path
                  ? true
                  : false
              }
              modelPath={settings?.whisper_model_path ?? null}
            />
          ) : step === 3 ? (
            <StepApiKey
              xaiKey={xaiKey}
              setXaiKey={setXaiKey}
              keyStatus={keyStatus}
              onSaveKey={onSaveKey}
            />
          ) : step === 4 ? (
            isWindows() ? (
              // Embedded LLM is Linux/macOS-only (issue #1). On Windows we
              // skip the GGUF download and explain the Ollama/cloud path.
              <div className="flex flex-col gap-4">
                <div>
                  <h3 className="text-lg font-semibold text-fg">
                    {t("wizard.windows_local.title")}
                  </h3>
                  <p className="text-sm text-fg-muted mt-1">
                    {t("wizard.windows_local.body")}
                  </p>
                  <p className="text-xs text-fg-faint mt-2">
                    {t("wizard.windows_local.hint")}
                  </p>
                </div>
              </div>
            ) : (
              <StepLlmDownload
                hardware={hardware}
                currentSlot={settings?.llm_default_slot ?? ""}
                onPickSlot={(s) => void onPickLlmSlot(s)}
                onDownload={onLlmDownload}
                downloadStarted={
                  llmStatus.kind !== "idle" || settings?.llm_model_path
                    ? true
                    : false
                }
                modelPath={settings?.llm_model_path ?? null}
              />
            )
          ) : (
            <StepFinish
              backend={backend}
              hardware={hardware}
              anyDownloadRunning={anyRunning}
            />
          )}
        </div>

        <div className="px-6 py-4 border-t border-outline flex justify-between items-center gap-2">
          <Button
            variant="ghost"
            onClick={() => setStep((s) => Math.max(1, (s - 1) as Step) as Step)}
            disabled={step === 1}
          >
            {t("wizard.nav.back")}
          </Button>
          {step < TOTAL_STEPS ? (
            <Button
              variant="secondary"
              onClick={() =>
                setStep((s) => Math.min(TOTAL_STEPS, (s + 1) as Step) as Step)
              }
            >
              {t("wizard.nav.next")}
            </Button>
          ) : (
            <Button onClick={() => void onFinish()}>
              {t("wizard.nav.finish")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );

  function StepDownload({
    onDownload,
    downloadStarted,
    modelPath,
  }: {
    onDownload: () => void;
    downloadStarted: boolean;
    modelPath: string | null;
  }): JSX.Element {
    return (
      <div className="flex flex-col gap-4">
        <Hero icon={<CloudDownloadIcon />} />
        <div>
          <h3 className="text-lg font-semibold text-fg">
            {t("wizard.download.title")}
          </h3>
          <p className="text-sm text-fg-muted mt-1">
            {t("wizard.download.intro_prefix")}{" "}
            <code className="text-brand font-mono">
              ggml-large-v3-turbo-q8_0.bin
            </code>{" "}
            {t("wizard.download.intro_middle")}{" "}
            <code className="text-brand font-mono">app_config_dir/models/</code>
            {t("wizard.download.intro_suffix")}
          </p>
          <p className="text-xs text-fg-faint mt-2">
            {t("wizard.download.hint_background")}
          </p>
        </div>
        <Button
          onClick={() => onDownload()}
          disabled={downloadStarted}
          className="self-start"
        >
          {modelPath
            ? t("wizard.download.btn.done")
            : downloadStarted
              ? t("wizard.download.btn.running")
              : t("wizard.download.btn.idle")}
        </Button>
        {modelPath ? (
          <div className="text-xs text-status-done">
            {t("wizard.download.configured", { path: modelPath })}
          </div>
        ) : null}
      </div>
    );
  }

  function StepApiKey({
    xaiKey,
    setXaiKey,
    keyStatus,
    onSaveKey,
  }: {
    xaiKey: string;
    setXaiKey: (v: string) => void;
    keyStatus:
      | null
      | { kind: "saving" }
      | { kind: "ok" }
      | { kind: "error"; msg: string };
    onSaveKey: () => Promise<void>;
  }): JSX.Element {
    return (
      <div className="flex flex-col gap-4">
        <Hero icon={<KeyIcon />} />
        <div>
          <h3 className="text-lg font-semibold text-fg">
            {t("wizard.api_key.title")}
          </h3>
          <p className="text-sm text-fg-muted mt-1">
            {t("wizard.api_key.intro_prefix")}{" "}
            <code className="text-brand font-mono">console.x.ai</code>
            {t("wizard.api_key.intro_suffix")}
          </p>
          <p className="text-xs text-fg-faint mt-1">
            {t("wizard.api_key.hint_keychain")}
          </p>
        </div>
        <Input
          type="password"
          value={xaiKey}
          onChange={(e) => setXaiKey(e.target.value)}
          placeholder={t("wizard.api_key.placeholder")}
          className="font-mono"
        />
        <div className="flex items-center gap-3">
          <Button
            onClick={() => void onSaveKey()}
            disabled={
              !xaiKey || (keyStatus !== null && keyStatus.kind === "saving")
            }
          >
            {keyStatus?.kind === "saving"
              ? t("wizard.api_key.btn.running")
              : t("wizard.api_key.btn.idle")}
          </Button>
          {keyStatus?.kind === "ok" ? (
            <span className="text-xs text-status-done">
              {t("wizard.api_key.ok")}
            </span>
          ) : null}
        </div>
        {keyStatus?.kind === "error" ? (
          <div className="text-xs text-status-error">{keyStatus.msg}</div>
        ) : null}
      </div>
    );
  }

  function StepLlmDownload({
    hardware,
    currentSlot,
    onPickSlot,
    onDownload,
    downloadStarted,
    modelPath,
  }: {
    hardware: HardwareReport | null;
    currentSlot: string;
    onPickSlot: (slot: string) => void;
    onDownload: () => void;
    downloadStarted: boolean;
    modelPath: string | null;
  }): JSX.Element {
    const rec = hardware ? recommendLlmSlot(hardware.total_ram_gb) : null;
    const mismatch = rec !== null && rec.slot !== currentSlot;

    const ramLabel =
      hardware && hardware.total_ram_gb > 0
        ? t("wizard.llm.recommendation_ram", {
            ram: hardware.total_ram_gb.toFixed(1),
          })
        : t("wizard.llm.recommendation_system");

    return (
      <div className="flex flex-col gap-4">
        <Hero icon={<CloudDownloadIcon />} />
        <div>
          <h3 className="text-lg font-semibold text-fg">
            {t("wizard.llm.title")}
          </h3>
          <p className="text-sm text-fg-muted mt-1">
            {t("wizard.llm.intro_prefix")}{" "}
            <code className="text-brand font-mono">llama-cpp-2</code>{" "}
            {t("wizard.llm.intro_middle")}{" "}
            <code className="text-brand font-mono">
              local_engine = &quot;embedded&quot;
            </code>{" "}
            {t("wizard.llm.intro_suffix")}
          </p>
          <p className="text-xs text-fg-faint mt-2">
            {t("wizard.llm.hint_background")}
          </p>
        </div>

        {rec && hardware ? (
          <div className="rounded-md bg-brand/10 border border-brand/30 px-3 py-2.5 text-sm">
            <div className="text-fg">
              <span className="text-fg-faint">
                {t("wizard.llm.recommendation_prefix")}{" "}
              </span>
              <span className="font-medium">{ramLabel}</span>:{" "}
              <span className="font-medium text-brand">{rec.label}</span>
            </div>
            {mismatch ? (
              <button
                type="button"
                onClick={() => onPickSlot(rec.slot)}
                className="mt-1.5 text-xs underline text-brand hover:text-brand-hover"
              >
                {t("wizard.llm.adopt")}
              </button>
            ) : (
              <div className="mt-1 text-xs text-status-recording">
                {t("wizard.llm.active")}
              </div>
            )}
          </div>
        ) : null}

        <Button
          onClick={() => onDownload()}
          disabled={downloadStarted}
          className="self-start"
        >
          {modelPath
            ? t("wizard.llm.btn.done")
            : downloadStarted
              ? t("wizard.llm.btn.running")
              : t("wizard.llm.btn.idle")}
        </Button>

        {modelPath ? (
          <div className="text-xs text-status-done">
            {t("wizard.llm.configured", { path: modelPath })}
          </div>
        ) : null}
      </div>
    );
  }

  function StepFinish({
    backend,
    hardware,
    anyDownloadRunning,
  }: {
    backend: WhisperBackendInfo | null;
    hardware: HardwareReport | null;
    anyDownloadRunning: boolean;
  }): JSX.Element {
    return (
      <div className="flex flex-col gap-4">
        <Hero icon={<CheckIcon />} accent="done" />
        {anyDownloadRunning ? (
          <Banner tone="warning">
            {t("wizard.finish.warning_downloads_running")}
          </Banner>
        ) : null}
        <div>
          <h3 className="text-lg font-semibold text-fg">
            {t("wizard.finish.title")}
          </h3>
          {backend ? (
            <div className="mt-3 rounded-md bg-elevated border border-outline p-3 text-sm">
              <div className="text-fg-muted text-xs mb-1">
                {t("wizard.finish.backend_label")}
              </div>
              <div className="text-status-done font-mono text-base">
                {t("wizard.finish.backend_value", {
                  backend: backend.backend,
                  factor: backend.expected_speedup.toFixed(1),
                })}
              </div>
              <div className="text-xs text-fg-faint mt-1">
                {backend.description}
              </div>
            </div>
          ) : null}
          {hardware && backend ? (
            <HardwareRecommendation
              hardware={hardware}
              active={backend.backend}
            />
          ) : null}
        </div>
        <p className="text-sm text-fg-muted">
          {t("wizard.finish.intro")}{" "}
          <kbd className="px-2 py-0.5 rounded-md bg-elevated border border-outline font-mono text-xs text-fg">
            {t("wizard.finish.kbd.hotkey")}
          </kbd>
          {t("wizard.finish.choose_mode")}{" "}
          <kbd className="px-2 py-0.5 rounded-md bg-elevated border border-outline font-mono text-xs text-fg">
            {t("wizard.finish.kbd.enter")}
          </kbd>
          {t("wizard.finish.same_stops")}
        </p>
        <ul className="text-sm text-fg-muted list-disc pl-5 flex flex-col gap-1">
          <li>
            {t("wizard.finish.bullet_modes_prefix")}{" "}
            <strong className="text-fg">
              {t("wizard.finish.bullet_modes_link")}
            </strong>
            {t("wizard.finish.bullet_modes_suffix")}
          </li>
          <li>
            {t("wizard.finish.bullet_custom_prefix")}{" "}
            <code className="text-brand font-mono">app_config_dir/modes/</code>
            {t("wizard.finish.bullet_custom_suffix")}
          </li>
          <li>
            {t("wizard.finish.bullet_logs_prefix")}{" "}
            <strong className="text-fg">
              {t("wizard.finish.bullet_logs_link")}
            </strong>
            {t("wizard.finish.bullet_logs_suffix")}
          </li>
        </ul>
      </div>
    );
  }
}

interface MiniProgressStackProps {
  whisperStatus: DownloadStatus;
  whisperProgress: ModelDownloadProgress | null;
  whisperFlashVisible: boolean;
  llmStatus: DownloadStatus;
  llmProgress: ModelDownloadProgress | null;
  llmFlashVisible: boolean;
}

/**
 * Sticky mini progress-bar stack in the wizard header. Per running
 * download it shows a thin bar with a provider tag + percent on the
 * right; on completion a brief green check, then hidden. The error
 * state replaces the bar with a {@link Banner}.
 *
 * TODO: Download cancellation is not available backend-side — once
 * `ipcCancelModelDownload` exists, add a × button per line here.
 */
function MiniProgressStack({
  whisperStatus,
  whisperProgress,
  whisperFlashVisible,
  llmStatus,
  llmProgress,
  llmFlashVisible,
}: MiniProgressStackProps): JSX.Element | null {
  const t = useT();
  const whisperRow = renderRow(
    t,
    "whisper",
    whisperStatus,
    whisperProgress,
    whisperFlashVisible,
  );
  const llmRow = renderRow(t, "llm", llmStatus, llmProgress, llmFlashVisible);
  if (whisperRow === null && llmRow === null) return null;
  return (
    <div className="mt-3 flex flex-col gap-1.5">
      {whisperRow}
      {llmRow}
    </div>
  );
}

type ProgressTag = "whisper" | "llm";

function renderRow(
  t: TranslateFn,
  tag: ProgressTag,
  status: DownloadStatus,
  progress: ModelDownloadProgress | null,
  flashVisible: boolean,
): JSX.Element | null {
  if (status.kind === "idle") return null;
  if (status.kind === "done") {
    if (!flashVisible) return null;
    return (
      <div
        key={tag}
        className="flex items-center gap-2 text-xs text-status-done"
        role="status"
      >
        <div className="h-1 flex-1 rounded-full bg-status-done/40 overflow-hidden">
          <div className="h-full w-full bg-status-done" />
        </div>
        <span className="font-mono">{t(`wizard.progress.${tag}_done`)}</span>
      </div>
    );
  }
  if (status.kind === "error") {
    return (
      <Banner key={tag} tone="error" dense>
        {t(`wizard.progress.${tag}_failed`, { message: status.msg })}
      </Banner>
    );
  }
  const pct =
    progress && progress.total
      ? Math.round((progress.downloaded / progress.total) * 100)
      : null;
  const ariaLabel =
    pct !== null
      ? t(`wizard.progress.${tag}_aria`, { pct })
      : t(`wizard.progress.${tag}_aria_pending`);
  const statusText =
    pct !== null
      ? t(`wizard.progress.${tag}_running`, { pct })
      : t(`wizard.progress.${tag}_pending`);
  return (
    <div
      key={tag}
      className="flex items-center gap-2 text-xs text-fg-muted"
      role="status"
      aria-label={ariaLabel}
    >
      <div className="h-1 flex-1 rounded-full bg-elevated overflow-hidden">
        {pct !== null ? (
          <div
            className="h-full bg-brand transition-all"
            style={{ width: `${pct}%` }}
          />
        ) : (
          <div className="h-full w-1/4 bg-brand/40 animate-pulse" />
        )}
      </div>
      <span className="font-mono whitespace-nowrap">{statusText}</span>
    </div>
  );
}

function StepWelcome(): JSX.Element {
  const t = useT();
  return (
    <div className="flex flex-col gap-4">
      <Hero icon={<Logo className="h-7 w-7" />} />
      <div>
        <h3 className="text-lg font-semibold text-fg">
          {t("wizard.welcome.title")}
        </h3>
        <p className="text-sm text-fg-muted mt-1">
          {t("wizard.welcome.intro")}
        </p>
      </div>
      <ul className="text-sm text-fg-muted list-disc pl-5 flex flex-col gap-1.5">
        <li>
          <strong className="text-fg">
            {t("wizard.welcome.bullet_local_emphasis")}
          </strong>{" "}
          {t("wizard.welcome.bullet_local")}
        </li>
        <li>
          <strong className="text-fg">
            {t("wizard.welcome.bullet_cloud_emphasis")}
          </strong>{" "}
          {t("wizard.welcome.bullet_cloud")}
        </li>
        <li>{t("wizard.welcome.bullet_modes")}</li>
      </ul>
      <p className="text-xs text-fg-faint">
        {t("wizard.welcome.footnote_setup")}
      </p>
      <p className="text-xs text-fg-faint">
        {t("wizard.welcome.footnote_relaunch_prefix")}{" "}
        <em className="not-italic text-fg-muted">
          {t("wizard.welcome.footnote_relaunch_path")}
        </em>
        {t("wizard.welcome.footnote_relaunch_suffix")}
      </p>
    </div>
  );
}

interface HardwareRecommendationProps {
  hardware: HardwareReport;
  active: string;
}

function HardwareRecommendation({
  hardware,
  active,
}: HardwareRecommendationProps): JSX.Element | null {
  const t = useT();
  const { recommended_variant, recommended_speedup } = hardware;
  if (recommended_variant === active) {
    return (
      <div className="mt-3 rounded-md bg-status-done/10 border border-status-done/40 p-3 text-xs text-status-done">
        {t("wizard.hw_rec.optimal")}
      </div>
    );
  }
  const variantLabel = t(`wizard.variant.${recommended_variant}`);
  return (
    <div className="mt-3 rounded-md bg-status-processing/10 border border-status-processing/40 p-3 text-xs text-status-processing flex flex-col gap-1.5">
      <div>
        <strong>{t("wizard.hw_rec.recommendation_label")}</strong>{" "}
        {t("wizard.hw_rec.body", {
          label: variantLabel,
          factor: recommended_speedup.toFixed(1),
        })}
      </div>
      <div className="text-fg-faint">
        {t("wizard.hw_rec.detected_prefix", {
          cores: hardware.cpu_logical_cores,
        })}
        {hardware.has_vulkan ? t("wizard.hw_rec.detected_vulkan") : ""}
        {hardware.has_nvidia_gpu ? t("wizard.hw_rec.detected_nvidia") : ""}
        {hardware.has_amd_gpu ? t("wizard.hw_rec.detected_amd") : ""}
        {hardware.is_apple_silicon ? t("wizard.hw_rec.detected_apple") : ""}.
      </div>
      <div className="text-fg-muted">
        {t("wizard.hw_rec.future_bundle", { variant: recommended_variant })}
      </div>
    </div>
  );
}

function StepIndicator({
  current,
  total,
}: {
  current: number;
  total: number;
}): JSX.Element {
  const t = useT();
  return (
    <div
      className="flex items-center gap-1.5"
      aria-label={t("wizard.indicator.aria")}
    >
      {Array.from({ length: total }, (_, i) => i + 1).map((n) => {
        const state =
          n < current ? "done" : n === current ? "active" : "pending";
        return (
          <div
            key={n}
            className={
              "h-1 flex-1 rounded-full transition-colors " +
              (state === "done"
                ? "bg-brand/60"
                : state === "active"
                  ? "bg-brand"
                  : "bg-elevated")
            }
            aria-current={state === "active" ? "step" : undefined}
          />
        );
      })}
    </div>
  );
}

function Hero({
  icon,
  accent = "brand",
}: {
  icon: JSX.Element;
  accent?: "brand" | "done";
}): JSX.Element {
  const bg = accent === "done" ? "bg-status-done/10" : "bg-brand/10";
  const fg = accent === "done" ? "text-status-done" : "text-brand";
  return (
    <div
      className={`self-start inline-flex items-center justify-center h-14 w-14 rounded-xl ${bg} ${fg}`}
      aria-hidden
    >
      {icon}
    </div>
  );
}

function CloudDownloadIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-7 w-7"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M6 17a4 4 0 1 1 .5-7.97A6 6 0 0 1 18 9a4 4 0 0 1 .5 8" />
      <path d="M12 13v8M8 17l4 4 4-4" />
    </svg>
  );
}

function KeyIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-7 w-7"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="8" cy="14" r="4" />
      <path d="M11 11l9-9M16 6l3 3M14 8l3 3" />
    </svg>
  );
}

function CheckIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-7 w-7"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M8 12l3 3 5-5" />
    </svg>
  );
}
