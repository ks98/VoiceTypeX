// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
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
import { useSettingsStore } from "../store";
import Button from "./Button";
import Logo from "./Logo";

type Step = 1 | 2 | 3 | 4 | 5;
const TOTAL_STEPS = 5;

interface OnboardingWizardProps {
  onClose: () => void;
}

const inputCls =
  "bg-surface border border-outline rounded-md px-3 py-2 text-sm text-fg placeholder:text-fg-faint focus:outline-none focus:border-brand focus:ring-1 focus:ring-brand/40";

export default function OnboardingWizard({
  onClose,
}: OnboardingWizardProps): JSX.Element {
  const settings = useSettingsStore((s) => s.settings);
  const update = useSettingsStore((s) => s.update);

  const [step, setStep] = useState<Step>(1);

  const [downloading, setDownloading] = useState(false);
  const [downloadProgress, setDownloadProgress] =
    useState<ModelDownloadProgress | null>(null);
  const [downloadError, setDownloadError] = useState<string | null>(null);

  const [llmDownloading, setLlmDownloading] = useState(false);
  const [llmDownloadProgress, setLlmDownloadProgress] =
    useState<ModelDownloadProgress | null>(null);
  const [llmDownloadError, setLlmDownloadError] = useState<string | null>(null);

  const [xaiKey, setXaiKey] = useState("");
  const [keyStatus, setKeyStatus] = useState<
    null | { kind: "saving" } | { kind: "ok" } | { kind: "error"; msg: string }
  >(null);

  const [backend, setBackend] = useState<WhisperBackendInfo | null>(null);
  const [hardware, setHardware] = useState<HardwareReport | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    void listen<ModelDownloadProgress>("model-download-progress", (event) =>
      setDownloadProgress(event.payload),
    ).then((fn) => unlistens.push(fn));
    void listen<ModelDownloadProgress>("llm-model-download-progress", (event) =>
      setLlmDownloadProgress(event.payload),
    ).then((fn) => unlistens.push(fn));
    void ipcGetWhisperBackend()
      .then(setBackend)
      .catch(() => null);
    void ipcGetHardwareReport()
      .then(setHardware)
      .catch(() => null);
    return () => {
      unlistens.forEach((u) => u());
    };
  }, []);

  const onDownload = async () => {
    setDownloading(true);
    setDownloadError(null);
    setDownloadProgress(null);
    try {
      const path = await ipcDownloadDefaultModel();
      void update({ whisper_model_path: path });
      setStep(3);
    } catch (e) {
      setDownloadError(String(e));
    } finally {
      setDownloading(false);
    }
  };

  const onPickLlmSlot = async (slot: string) => {
    await update({ llm_default_slot: slot });
  };

  const onLlmDownload = async () => {
    setLlmDownloading(true);
    setLlmDownloadError(null);
    setLlmDownloadProgress(null);
    try {
      await ipcDownloadLlmDefaultModel();
    } catch (e) {
      setLlmDownloadError(String(e));
    } finally {
      setLlmDownloading(false);
    }
  };

  const onSaveKey = async () => {
    setKeyStatus({ kind: "saving" });
    try {
      await ipcSetProviderKey("xai", xaiKey);
      try {
        await ipcTestProviderConnection("xai");
        setKeyStatus({ kind: "ok" });
      } catch (e) {
        setKeyStatus({ kind: "error", msg: `Test fehlgeschlagen: ${e}` });
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

  const fmtMb = (bytes: number) => `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  const progressPct =
    downloadProgress && downloadProgress.total
      ? Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)
      : null;

  return (
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-surface border border-outline rounded-xl max-w-2xl w-full overflow-auto shadow-2xl">
        <div className="px-6 pt-6 pb-4 border-b border-outline">
          <div className="flex justify-between items-start mb-4">
            <div>
              <h2 className="text-xl font-semibold text-fg tracking-tight">
                Willkommen bei VoiceTypeX
              </h2>
              <div className="text-xs text-fg-faint mt-0.5">
                Setup in {TOTAL_STEPS} kurzen Schritten
              </div>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void skipAll()}
            >
              überspringen
            </Button>
          </div>
          <StepIndicator current={step} total={TOTAL_STEPS} />
        </div>

        <div className="px-6 py-8 min-h-[360px]">
          {step === 1 ? (
            <StepWelcome />
          ) : step === 2 ? (
            <StepDownload
              onDownload={onDownload}
              downloading={downloading}
              progressPct={progressPct}
              downloadProgress={downloadProgress}
              downloadError={downloadError}
              fmtMb={fmtMb}
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
            <StepLlmDownload
              hardware={hardware}
              currentSlot={settings?.llm_default_slot ?? ""}
              onPickSlot={(s) => void onPickLlmSlot(s)}
              onDownload={onLlmDownload}
              downloading={llmDownloading}
              progress={llmDownloadProgress}
              error={llmDownloadError}
              modelPath={settings?.llm_model_path ?? null}
              fmtMb={fmtMb}
            />
          ) : (
            <StepFinish backend={backend} hardware={hardware} />
          )}
        </div>

        <div className="px-6 py-4 border-t border-outline flex justify-between items-center gap-2">
          <Button
            variant="ghost"
            onClick={() => setStep((s) => Math.max(1, (s - 1) as Step) as Step)}
            disabled={step === 1}
          >
            ← Zurück
          </Button>
          {step < TOTAL_STEPS ? (
            <Button
              variant="secondary"
              onClick={() =>
                setStep(
                  (s) => Math.min(TOTAL_STEPS, (s + 1) as Step) as Step,
                )
              }
            >
              Weiter →
            </Button>
          ) : (
            <Button onClick={() => void onFinish()}>
              Setup abschließen
            </Button>
          )}
        </div>
      </div>
    </div>
  );

  function StepDownload({
    onDownload,
    downloading,
    progressPct,
    downloadProgress,
    downloadError,
    fmtMb,
    modelPath,
  }: {
    onDownload: () => Promise<void>;
    downloading: boolean;
    progressPct: number | null;
    downloadProgress: ModelDownloadProgress | null;
    downloadError: string | null;
    fmtMb: (bytes: number) => string;
    modelPath: string | null;
  }): JSX.Element {
    return (
      <div className="flex flex-col gap-4">
        <Hero icon={<CloudDownloadIcon />} />
        <div>
          <h3 className="text-lg font-semibold text-fg">
            Lokales Whisper-Modell
          </h3>
          <p className="text-sm text-fg-muted mt-1">
            Default ist{" "}
            <code className="text-brand font-mono">
              ggml-large-v3-turbo-q5_0.bin
            </code>{" "}
            (~547 MB). Beste Balance aus deutscher Erkennung und CPU-Latenz.
            Wird einmalig nach{" "}
            <code className="text-brand font-mono">app_data_dir/models/</code>{" "}
            heruntergeladen.
          </p>
        </div>
        <Button
          onClick={() => void onDownload()}
          disabled={downloading}
          className="self-start"
        >
          {downloading
            ? "Lade Modell…"
            : "Default-Modell jetzt herunterladen"}
        </Button>
        {downloadProgress ? (
          <div className="flex flex-col gap-1.5 text-xs text-fg-muted">
            <div>
              {fmtMb(downloadProgress.downloaded)}
              {downloadProgress.total
                ? ` von ${fmtMb(downloadProgress.total)}`
                : ""}
              {progressPct !== null ? ` (${progressPct} %)` : ""}
            </div>
            {progressPct !== null ? (
              <div className="h-2 bg-elevated rounded-full overflow-hidden">
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
        {modelPath ? (
          <div className="text-xs text-status-done">
            ✓ Modell konfiguriert: {modelPath}
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
            xAI-API-Key (optional)
          </h3>
          <p className="text-sm text-fg-muted mt-1">
            xAI bietet hochwertige Cloud-STT (Grok-STT) und LLM (Grok-4)
            mit demselben Key. Für die Modi „E-Mail", „Slack/Teams",
            „GitHub Issue" und „Anweisung an Coding-Agent" benötigt. Account und
            Key:{" "}
            <code className="text-brand font-mono">console.x.ai</code>.
          </p>
          <p className="text-xs text-fg-faint mt-1">
            Der Key wird im OS-Keychain gespeichert, niemals als Klartext
            auf Disk. Du kannst diesen Schritt überspringen und nur die
            lokalen Modi nutzen.
          </p>
        </div>
        <input
          type="password"
          value={xaiKey}
          onChange={(e) => setXaiKey(e.target.value)}
          placeholder="xai-…"
          className={`${inputCls} font-mono`}
        />
        <div className="flex items-center gap-3">
          <Button
            onClick={() => void onSaveKey()}
            disabled={
              !xaiKey ||
              (keyStatus !== null && keyStatus.kind === "saving")
            }
          >
            {keyStatus?.kind === "saving"
              ? "Speichere + teste…"
              : "Speichern + Verbindung testen"}
          </Button>
          {keyStatus?.kind === "ok" ? (
            <span className="text-xs text-status-done">
              ✓ Verbindung erfolgreich
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
    downloading,
    progress,
    error,
    modelPath,
    fmtMb,
  }: {
    hardware: HardwareReport | null;
    currentSlot: string;
    onPickSlot: (slot: string) => void;
    onDownload: () => Promise<void>;
    downloading: boolean;
    progress: ModelDownloadProgress | null;
    error: string | null;
    modelPath: string | null;
    fmtMb: (bytes: number) => string;
  }): JSX.Element {
    const rec = hardware ? recommendLlmSlot(hardware.total_ram_gb) : null;
    const mismatch = rec !== null && rec.slot !== currentSlot;
    const pct =
      progress && progress.total
        ? Math.round((progress.downloaded / progress.total) * 100)
        : null;

    return (
      <div className="flex flex-col gap-4">
        <Hero icon={<CloudDownloadIcon />} />
        <div>
          <h3 className="text-lg font-semibold text-fg">
            Lokales LLM-Modell (optional)
          </h3>
          <p className="text-sm text-fg-muted mt-1">
            VoiceTypeX bringt einen eingebetteten LLM-Pfad via{" "}
            <code className="text-brand font-mono">llama-cpp-2</code> — kein
            externer Daemon nötig. Wird genutzt von Modi mit{" "}
            <code className="text-brand font-mono">
              local_engine = "embedded"
            </code>{" "}
            in der Mode-TOML. Wenn du nur Cloud-Modi nutzt, kannst du diesen
            Schritt überspringen.
          </p>
        </div>

        {rec && hardware ? (
          <div className="rounded-md bg-brand/10 border border-brand/30 px-3 py-2.5 text-sm">
            <div className="text-fg">
              <span className="text-fg-faint">Empfehlung für </span>
              <span className="font-medium">
                {hardware.total_ram_gb > 0
                  ? `${hardware.total_ram_gb.toFixed(1)} GB RAM`
                  : "dein System"}
              </span>
              :{" "}
              <span className="font-medium text-brand">{rec.label}</span>
            </div>
            {mismatch ? (
              <button
                type="button"
                onClick={() => onPickSlot(rec.slot)}
                className="mt-1.5 text-xs underline text-brand hover:text-brand-hover"
              >
                Empfehlung übernehmen
              </button>
            ) : (
              <div className="mt-1 text-xs text-status-recording">
                ✓ aktiv
              </div>
            )}
          </div>
        ) : null}

        <Button
          onClick={() => void onDownload()}
          disabled={downloading}
          className="self-start"
        >
          {downloading
            ? "Lade LLM-Modell…"
            : "LLM-Modell jetzt herunterladen"}
        </Button>

        {progress ? (
          <div className="flex flex-col gap-1.5 text-xs text-fg-muted">
            <div>
              {fmtMb(progress.downloaded)}
              {progress.total ? ` von ${fmtMb(progress.total)}` : ""}
              {pct !== null ? ` (${pct} %)` : ""}
            </div>
            {pct !== null ? (
              <div className="h-2 bg-elevated rounded-full overflow-hidden">
                <div
                  className="h-full bg-brand transition-all"
                  style={{ width: `${pct}%` }}
                />
              </div>
            ) : null}
          </div>
        ) : null}

        {error ? (
          <div className="text-xs text-status-error">{error}</div>
        ) : null}

        {modelPath ? (
          <div className="text-xs text-status-done">
            ✓ LLM-Modell konfiguriert: {modelPath}
          </div>
        ) : null}
      </div>
    );
  }

  function StepFinish({
    backend,
    hardware,
  }: {
    backend: WhisperBackendInfo | null;
    hardware: HardwareReport | null;
  }): JSX.Element {
    return (
      <div className="flex flex-col gap-4">
        <Hero icon={<CheckIcon />} accent="done" />
        <div>
          <h3 className="text-lg font-semibold text-fg">
            System-Check &amp; Fertig
          </h3>
          {backend ? (
            <div className="mt-3 rounded-md bg-elevated border border-outline p-3 text-sm">
              <div className="text-fg-muted text-xs mb-1">
                Whisper-Backend dieser Variante
              </div>
              <div className="text-status-done font-mono text-base">
                {backend.backend} (~{backend.expected_speedup.toFixed(1)}×)
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
          Du kannst jetzt diktieren. Drück{" "}
          <kbd className="px-2 py-0.5 rounded-md bg-elevated border border-outline font-mono text-xs text-fg">
            Ctrl+Alt+Space
          </kbd>
          , wähle mit den Pfeiltasten einen Modus und bestätige mit{" "}
          <kbd className="px-2 py-0.5 rounded-md bg-elevated border border-outline font-mono text-xs text-fg">
            Enter
          </kbd>
          . Derselbe Hotkey stoppt die Aufnahme.
        </p>
        <ul className="text-sm text-fg-muted list-disc pl-5 flex flex-col gap-1">
          <li>
            Modus-Liste anpassen: Tab{" "}
            <strong className="text-fg">Modi</strong>.
          </li>
          <li>
            Eigene Modi: UI oder TOML in{" "}
            <code className="text-brand font-mono">app_config_dir/modes/</code>.
          </li>
          <li>
            Diagnose-Logs: Tab <strong className="text-fg">Logs</strong>.
          </li>
        </ul>
      </div>
    );
  }
}

function StepWelcome(): JSX.Element {
  return (
    <div className="flex flex-col gap-4">
      <Hero icon={<Logo className="h-7 w-7" />} />
      <div>
        <h3 className="text-lg font-semibold text-fg">Was ist VoiceTypeX?</h3>
        <p className="text-sm text-fg-muted mt-1">
          VoiceTypeX nimmt deine Stimme per Hotkey auf, transkribiert sie
          (lokal oder via Cloud) und fügt den Text an der aktuellen
          Cursor-Position ein.
        </p>
      </div>
      <ul className="text-sm text-fg-muted list-disc pl-5 flex flex-col gap-1.5">
        <li>
          <strong className="text-fg">Lokal:</strong> 100 % offline via
          whisper.cpp — kostenlos, deine Audio-Daten verlassen niemals den
          Rechner.
        </li>
        <li>
          <strong className="text-fg">Cloud:</strong> xAI &amp; andere
          Provider (BYOK) — höhere Qualität, du bringst deinen eigenen
          API-Key mit.
        </li>
        <li>
          Sechs Standard-Modi vorinstalliert (E-Mail, Slack, Issue …),
          beliebig viele eigene möglich.
        </li>
      </ul>
      <p className="text-xs text-fg-faint">
        Das Setup dauert &lt; 5 Minuten. Du kannst Schritte überspringen
        und später aus den Einstellungen nachholen.
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
  const { recommended_variant, recommended_speedup } = hardware;
  if (recommended_variant === active) {
    return (
      <div className="mt-3 rounded-md bg-status-done/10 border border-status-done/40 p-3 text-xs text-status-done">
        ✓ Diese Variante ist optimal für deine Hardware. Kein Upgrade nötig.
      </div>
    );
  }
  const variantLabel: Record<string, string> = {
    cpu: "CPU-only",
    openblas: "OpenBLAS (CPU-BLAS)",
    vulkan: "Vulkan (cross-platform GPU)",
    cuda: "CUDA (NVIDIA-GPU)",
    metal: "Metal (Apple Silicon)",
    coreml: "CoreML (Apple Silicon)",
  };
  return (
    <div className="mt-3 rounded-md bg-status-processing/10 border border-status-processing/40 p-3 text-xs text-status-processing flex flex-col gap-1.5">
      <div>
        <strong>Empfehlung:</strong> deine Hardware unterstützt{" "}
        <span className="font-mono">
          {variantLabel[recommended_variant]}
        </span>{" "}
        — eine separate Variante könnte hier ~
        {recommended_speedup.toFixed(1)}× schneller transkribieren.
      </div>
      <div className="text-fg-faint">
        Detected: CPU {hardware.cpu_logical_cores} Cores
        {hardware.has_vulkan ? ", Vulkan" : ""}
        {hardware.has_nvidia_gpu ? ", NVIDIA-GPU" : ""}
        {hardware.has_amd_gpu ? ", AMD-GPU" : ""}
        {hardware.is_apple_silicon ? ", Apple-Silicon" : ""}.
      </div>
      <div className="text-fg-muted">
        Ab dem ersten offiziellen Release wird die {recommended_variant}-
        Variante als separater Bundle-Download bereitstehen.
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
  return (
    <div className="flex items-center gap-1.5" aria-label="Fortschritt">
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
