// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  ipcDownloadDefaultModel,
  ipcSetProviderKey,
  ipcTestProviderConnection,
  type ModelDownloadProgress,
} from "../lib/tauri";
import { useSettingsStore } from "../store";

type Step = 1 | 2 | 3 | 4;

interface OnboardingWizardProps {
  onClose: () => void;
}

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

  const [xaiKey, setXaiKey] = useState("");
  const [keyStatus, setKeyStatus] = useState<
    null | { kind: "saving" } | { kind: "ok" } | { kind: "error"; msg: string }
  >(null);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen<ModelDownloadProgress>("model-download-progress", (event) =>
      setDownloadProgress(event.payload),
    ).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
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
    <div className="fixed inset-0 bg-black/70 flex items-center justify-center z-50 p-4">
      <div className="bg-slate-950 border border-slate-700 rounded-lg max-w-2xl w-full overflow-auto">
        <div className="p-6 border-b border-slate-800 flex justify-between items-center">
          <div>
            <h2 className="text-xl font-semibold text-brand-500">
              Willkommen bei VoiceTypeX
            </h2>
            <div className="text-xs text-slate-500 mt-1">
              Schritt {step} von 4
            </div>
          </div>
          <button
            type="button"
            onClick={() => void skipAll()}
            className="text-xs text-slate-500 hover:text-slate-300"
          >
            Setup ueberspringen
          </button>
        </div>

        <div className="p-6 min-h-[320px]">
          {step === 1 ? (
            <div className="flex flex-col gap-4">
              <h3 className="text-lg font-medium text-slate-100">
                Was ist VoiceTypeX?
              </h3>
              <p className="text-sm text-slate-300">
                VoiceTypeX nimmt deine Stimme per Hotkey auf, transkribiert sie
                (lokal oder via Cloud) und fuegt den Text an der aktuellen
                Cursor-Position ein.
              </p>
              <ul className="text-sm text-slate-400 list-disc pl-5 flex flex-col gap-1">
                <li>
                  <strong className="text-slate-200">Lokal:</strong> 100 %
                  offline via whisper.cpp — kostenlos, deine Audio-Daten
                  verlassen niemals den Rechner.
                </li>
                <li>
                  <strong className="text-slate-200">Cloud:</strong> xAI &amp;
                  andere Provider (BYOK) — hoehere Qualitaet, du bringst deinen
                  eigenen API-Key mit.
                </li>
                <li>
                  Sechs Standard-Modi vorinstalliert (E-Mail, Slack, Issue …),
                  beliebig viele eigene moeglich.
                </li>
              </ul>
              <p className="text-xs text-slate-500">
                Das Setup dauert &lt; 5 Minuten. Du kannst Schritte
                ueberspringen und spaeter aus den Einstellungen nachholen.
              </p>
            </div>
          ) : null}

          {step === 2 ? (
            <div className="flex flex-col gap-4">
              <h3 className="text-lg font-medium text-slate-100">
                Lokales Whisper-Modell
              </h3>
              <p className="text-sm text-slate-300">
                Default ist{" "}
                <code className="text-brand-500">
                  ggml-large-v3-turbo-q5_0.bin
                </code>{" "}
                (~547 MB). Beste Balance aus deutscher Erkennung und CPU-Latenz.
                Wird einmalig nach{" "}
                <code className="text-brand-500">app_data_dir/models/</code>{" "}
                heruntergeladen.
              </p>
              <button
                type="button"
                onClick={() => void onDownload()}
                disabled={downloading}
                className="self-start px-4 py-2 rounded bg-brand-700 hover:bg-brand-500 disabled:bg-slate-800 disabled:text-slate-500 text-sm"
              >
                {downloading
                  ? "Lade Modell…"
                  : "Default-Modell jetzt herunterladen"}
              </button>
              {downloadProgress ? (
                <div className="flex flex-col gap-1 text-xs text-slate-400">
                  <div>
                    {fmtMb(downloadProgress.downloaded)}
                    {downloadProgress.total
                      ? ` von ${fmtMb(downloadProgress.total)}`
                      : ""}
                    {progressPct !== null ? ` (${progressPct} %)` : ""}
                  </div>
                  {progressPct !== null ? (
                    <div className="h-2 bg-slate-800 rounded overflow-hidden">
                      <div
                        className="h-full bg-brand-500 transition-all"
                        style={{ width: `${progressPct}%` }}
                      />
                    </div>
                  ) : null}
                </div>
              ) : null}
              {downloadError ? (
                <div className="text-xs text-red-400">{downloadError}</div>
              ) : null}
              {settings?.whisper_model_path ? (
                <div className="text-xs text-emerald-400">
                  ✓ Modell konfiguriert: {settings.whisper_model_path}
                </div>
              ) : null}
            </div>
          ) : null}

          {step === 3 ? (
            <div className="flex flex-col gap-4">
              <h3 className="text-lg font-medium text-slate-100">
                xAI-API-Key (optional)
              </h3>
              <p className="text-sm text-slate-300">
                xAI bietet hochwertige Cloud-STT (Grok-STT) und LLM (Grok-4) mit
                demselben Key. Fuer die Modi „E-Mail", „Slack", „Issue" und
                „Claude-Code-Anweisung" benoetigt. Account und Key:{" "}
                <code className="text-brand-500">console.x.ai</code>.
              </p>
              <p className="text-xs text-slate-500">
                Der Key wird im OS-Keychain gespeichert, niemals als Klartext
                auf Disk. Du kannst diesen Schritt ueberspringen und nur die
                lokalen Modi nutzen.
              </p>
              <input
                type="password"
                value={xaiKey}
                onChange={(e) => setXaiKey(e.target.value)}
                placeholder="xai-…"
                className="bg-slate-900 border border-slate-700 rounded px-3 py-2 text-sm font-mono"
              />
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => void onSaveKey()}
                  disabled={
                    !xaiKey ||
                    (keyStatus !== null && keyStatus.kind === "saving")
                  }
                  className="px-4 py-2 rounded bg-brand-700 hover:bg-brand-500 disabled:bg-slate-800 disabled:text-slate-500 text-sm"
                >
                  {keyStatus?.kind === "saving"
                    ? "Speichere + teste…"
                    : "Speichern + Verbindung testen"}
                </button>
                {keyStatus?.kind === "ok" ? (
                  <span className="text-xs text-emerald-400">
                    ✓ Verbindung erfolgreich
                  </span>
                ) : null}
              </div>
              {keyStatus?.kind === "error" ? (
                <div className="text-xs text-red-400">{keyStatus.msg}</div>
              ) : null}
            </div>
          ) : null}

          {step === 4 ? (
            <div className="flex flex-col gap-4">
              <h3 className="text-lg font-medium text-slate-100">Fertig!</h3>
              <p className="text-sm text-slate-300">
                Du kannst jetzt diktieren. Drueck{" "}
                <kbd className="px-2 py-0.5 rounded bg-slate-800 font-mono text-xs">
                  Ctrl+Alt+D
                </kbd>{" "}
                fuer ein lokales Diktat (Modus „Exaktes Diktat").
              </p>
              <ul className="text-sm text-slate-400 list-disc pl-5 flex flex-col gap-1">
                <li>
                  Die Hotkeys aller Modi siehst du im Tab{" "}
                  <strong className="text-slate-200">Modi</strong>.
                </li>
                <li>
                  Eigene Modi kannst du im UI erstellen oder direkt als TOML in{" "}
                  <code className="text-brand-500">app_config_dir/modes/</code>{" "}
                  ablegen.
                </li>
                <li>
                  Diagnose-Logs im Tab{" "}
                  <strong className="text-slate-200">Logs</strong>.
                </li>
              </ul>
            </div>
          ) : null}
        </div>

        <div className="p-5 border-t border-slate-800 flex justify-between gap-2">
          <button
            type="button"
            onClick={() => setStep((s) => Math.max(1, (s - 1) as Step) as Step)}
            disabled={step === 1}
            className="text-xs text-slate-400 hover:text-slate-200 disabled:opacity-30"
          >
            ← Zurueck
          </button>
          {step < 4 ? (
            <button
              type="button"
              onClick={() =>
                setStep((s) => Math.min(4, (s + 1) as Step) as Step)
              }
              className="px-4 py-2 rounded bg-slate-800 hover:bg-slate-700 text-sm"
            >
              Weiter →
            </button>
          ) : (
            <button
              type="button"
              onClick={() => void onFinish()}
              className="px-4 py-2 rounded bg-brand-700 hover:bg-brand-500 text-sm"
            >
              Setup abschliessen
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
