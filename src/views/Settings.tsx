// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import Field from "../components/Field";
import Button from "../components/Button";
import ApiKeysSection from "../components/ApiKeysSection";
import TestTranscriptionSection from "../components/TestTranscriptionSection";
import AutoPasteTestSection from "../components/AutoPasteTestSection";
import ThemeToggle from "../components/ThemeToggle";
import { useSettingsStore } from "../store";
import {
  ipcDownloadDefaultModel,
  ipcDownloadLlmDefaultModel,
  ipcGetEffectiveMenuHotkey,
  ipcGetHardwareReport,
  ipcGetSessionInfo,
  ipcGetWhisperBackend,
  type HardwareReport,
  type ModelDownloadProgress,
  type SessionInfo,
  type WhisperBackendInfo,
} from "../lib/tauri";
import { recommendLlmSlot } from "../lib/recommend";

const inputCls =
  "bg-surface border border-outline rounded-md px-3 py-2 text-sm text-fg placeholder:text-fg-faint focus:outline-none focus:border-brand focus:ring-1 focus:ring-brand/40";

export default function Settings(): JSX.Element {
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
    return <div className="text-fg-faint">Lade Einstellungen…</div>;
  }

  const onPickModel = async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Whisper-Modell (GGML)", extensions: ["bin", "gguf"] }],
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
      await load(); // Settings neu ziehen, damit llm_model_path aktualisiert ist
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

  const fmtMb = (bytes: number) => `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  const progressPct =
    progress && progress.total
      ? Math.round((progress.downloaded / progress.total) * 100)
      : null;

  return (
    <div className="flex flex-col gap-6 max-w-2xl">
      {error ? (
        <div className="rounded-md bg-status-error/10 border border-status-error/40 px-3 py-2 text-sm text-status-error">
          {error}
        </div>
      ) : null}

      <Field label="Erscheinungsbild" hint="System folgt der OS-Einstellung.">
        <ThemeToggle />
      </Field>

      <Field
        label="Audio-Eingabegeraet"
        hint="Leer = OS-Standard. Aenderungen wirken beim naechsten Recording."
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
          <option value="">— OS-Standard —</option>
          {audioDevices.map((d) => (
            <option key={d} value={d}>
              {d}
            </option>
          ))}
        </select>
      </Field>

      <HardwareStatusField
        hardware={hardware}
        activeBackend={activeBackend}
        currentLlmSlot={settings.llm_default_slot}
        onPickLlmSlot={(slot) => void update({ llm_default_slot: slot })}
      />

      <Field
        label="Lokales Whisper-Modell"
        hint="Default-Slot wird beim ersten Start nach app_data_dir/models/ heruntergeladen. Eigener Pfad ueberschreibt das."
      >
        <div className="flex gap-2">
          <input
            type="text"
            className={`${inputCls} flex-1`}
            placeholder="(Default-Modell aus Slot)"
            value={settings.whisper_model_path ?? ""}
            onChange={(e) =>
              void update({ whisper_model_path: e.target.value || null })
            }
          />
          <Button variant="secondary" onClick={() => void onPickModel()}>
            Datei waehlen…
          </Button>
        </div>
        <select
          className={inputCls}
          value={settings.whisper_default_slot}
          onChange={(e) =>
            void update({ whisper_default_slot: e.target.value })
          }
        >
          <option value="large-v3-turbo-q8_0">
            large-v3-turbo-q8_0 (~874 MB, Default Mai 2026)
          </option>
          <option value="large-v3-turbo-german-q5_0">
            large-v3-turbo-german-q5_0 (~574 MB, DE Pro · primeline)
          </option>
          <option value="large-v3-turbo-q5_0">
            large-v3-turbo-q5_0 (~547 MB, Light-Hardware)
          </option>
          <option value="small-q5_1">small-q5_1 (~181 MB, 4-GB-Geraete)</option>
          <option value="large-v3-turbo">
            large-v3-turbo (~1.6 GB, F16 Power-User)
          </option>
        </select>
        <div className="flex flex-col gap-1.5">
          <Button
            onClick={() => void onDownloadDefault()}
            disabled={downloading}
            className="self-start"
          >
            {downloading
              ? "Lade Modell…"
              : "Default-Modell jetzt herunterladen"}
          </Button>
          {progress ? (
            <div className="flex flex-col gap-1 text-xs text-fg-muted">
              <div>
                {fmtMb(progress.downloaded)}
                {progress.total ? ` von ${fmtMb(progress.total)}` : ""}
                {progressPct !== null ? ` (${progressPct} %)` : ""}
              </div>
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

      <MenuHotkeyField
        session={session}
        settingsValue={settings.menu_hotkey}
        effective={effectiveHotkey}
        onChange={(v) => void update({ menu_hotkey: v })}
      />

      <Field
        label="Whisper-Threads (lokales STT)"
        hint="Anzahl CPU-Threads fuer Whisper-Inferenz. Leer = automatisch (CPU-Cores, max 8). Niedrigere Werte schonen das System fuer parallele Arbeit, hoehere koennen schneller sein."
      >
        <input
          type="number"
          min={1}
          max={32}
          className={`${inputCls} w-32`}
          placeholder="auto"
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
        label="Ollama-Endpunkt"
        hint="Lokales LLM via externer Ollama-Daemon. Standardport ist 11434. Wird benutzt, wenn ein Modus local_engine = ollama setzt (Default fuer Backward-Compat)."
      >
        <input
          type="text"
          className={inputCls}
          value={settings.ollama_url}
          onChange={(e) => void update({ ollama_url: e.target.value })}
        />
      </Field>

      <Field
        label="Embedded LLM-Modell (Phase 3b)"
        hint="GGUF-Modell fuer den eingebetteten llama-cpp-2-Pfad. Wird ohne externen Daemon im VoiceTypeX-Prozess geladen. Aktiviert pro Modus via local_engine = embedded."
      >
        <select
          className={inputCls}
          value={settings.llm_default_slot}
          onChange={(e) =>
            void update({ llm_default_slot: e.target.value })
          }
        >
          <option value="gemma4-e4b-it-q5_k_m">
            Gemma 4 E4B-IT Q5_K_M (~5,1 GB, Pro · 12+ GB RAM · April 2026)
          </option>
          <option value="gemma4-e2b-it-q5_k_m">
            Gemma 4 E2B-IT Q5_K_M (~3,1 GB, Mittel · 8-12 GB RAM)
          </option>
          <option value="gemma3-1b-it-q5_k_m">
            Gemma 3 1B-IT Q5_K_M (~851 MB, Light · 4-GB-RAM-OK)
          </option>
          <option value="gemma3-4b-it-q5_k_m">
            Gemma 3 4B-IT Q5_K_M (~2,8 GB, Legacy-Pro · Maerz 2025)
          </option>
          <option value="llama3.2-1b-instruct-q5_k_m">
            Llama 3.2 1B-Instruct Q5_K_M (~912 MB, EN-fokussiert)
          </option>
          <option value="qwen2.5-1.5b-instruct-q5_k_m">
            Qwen 2.5 1.5B-Instruct Q5_K_M (~1,3 GB, Code-affin)
          </option>
        </select>
        <input
          type="text"
          className={inputCls}
          placeholder="(Default-Modell aus Slot)"
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
              ? "Lade LLM-Modell…"
              : "LLM-Modell jetzt herunterladen"}
          </Button>
          {llmProgress ? (
            <div className="flex flex-col gap-1 text-xs text-fg-muted">
              <div>
                {fmtMb(llmProgress.downloaded)}
                {llmProgress.total ? ` von ${fmtMb(llmProgress.total)}` : ""}
                {llmProgress.total
                  ? ` (${Math.round((llmProgress.downloaded / llmProgress.total) * 100)} %)`
                  : ""}
              </div>
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
            <div className="text-xs text-status-error">{llmDownloadError}</div>
          ) : null}
        </div>
      </Field>

      <Field
        label="Diagnose-Logging"
        hint="Erlaubt Audio-Metadata, Transkripte und LLM-Antworten in den Logs. Default OFF (Datenschutz)."
      >
        <label className="flex items-center gap-2 text-sm text-fg">
          <input
            type="checkbox"
            checked={settings.diagnostic_logging}
            onChange={(e) =>
              void update({ diagnostic_logging: e.target.checked })
            }
          />
          aktivieren
        </label>
      </Field>

      <Field
        label="Beim Login automatisch starten"
        hint="Default OFF. Tauri-Plugin-Autostart legt einen LaunchAgent bzw. Run-Eintrag an."
      >
        <label className="flex items-center gap-2 text-sm text-fg">
          <input
            type="checkbox"
            checked={settings.autostart}
            onChange={(e) => void update({ autostart: e.target.checked })}
          />
          aktivieren
        </label>
      </Field>

      <TestTranscriptionSection />

      <AutoPasteTestSection />

      <ApiKeysSection />
    </div>
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
  const isWayland = session?.display_server === "wayland";

  if (isWayland) {
    return (
      <Field
        label="Globaler Menue-Hotkey (Wayland)"
        hint="Auf Wayland verwaltet der Compositor (KDE / GNOME) die Tastenbindung. Aenderungen unter System Settings → Globale Verknuepfungen → VoiceTypeX. Der hier angezeigte Wert ist der aktuelle effektive Trigger; Aenderungen wirken nach App-Neustart."
      >
        <div className="bg-elevated border border-outline rounded-md px-3 py-2 text-sm font-mono w-72 text-fg-muted">
          {effective ?? settingsValue}
        </div>
      </Field>
    );
  }

  return (
    <Field
      label="Globaler Menue-Hotkey"
      hint="Genau ein Hotkey fuer die ganze App. Drueckst du ihn, oeffnet sich das Modus-Menue (Pfeile + Enter); waehrend einer laufenden Aufnahme stoppt derselbe Hotkey das Recording. Aenderungen wirken nach App-Neustart."
    >
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

/**
 * Hardware-Status — read-only Info-Panel zwischen Audio-Geraet und
 * Whisper-Modell. Zeigt was die App ueber die Hardware weiss, welchen
 * Compute-Backend sie im aktuellen Build verwendet, und ob ein anderer
 * Backend-Build mehr Speedup braechte.
 *
 * Quelle: `get_hardware_report` (Runtime-Detection von libvulkan,
 * libcuda, /proc/meminfo) + `get_whisper_backend` (Compile-Time-Feature).
 */
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
  if (!hardware && !activeBackend) {
    return (
      <Field
        label="Hardware-Status"
        hint="Wird beim App-Start ermittelt."
      >
        <div className="text-sm text-fg-faint">Lade Hardware-Info…</div>
      </Field>
    );
  }

  const ramText =
    hardware && hardware.total_ram_gb > 0
      ? `${hardware.total_ram_gb.toFixed(1)} GB (${hardware.available_ram_gb.toFixed(1)} GB frei)`
      : "—";

  const gpuFlags: string[] = [];
  if (hardware?.has_nvidia_gpu) gpuFlags.push("NVIDIA");
  if (hardware?.has_amd_gpu) gpuFlags.push("AMD");
  if (hardware?.is_apple_silicon) gpuFlags.push("Apple Silicon");
  if (gpuFlags.length === 0) gpuFlags.push("keine dedizierte GPU detektiert");

  const libsFlags: string[] = [];
  if (hardware?.has_vulkan) libsFlags.push("Vulkan");
  if (hardware?.has_openblas) libsFlags.push("OpenBLAS");
  if (libsFlags.length === 0) libsFlags.push("keine");

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
      label="Hardware-Status"
      hint="Was die App ueber dein System weiss. Read-only; basiert auf Runtime-Probing und Compile-Time-Feature."
    >
      <div className="flex flex-col gap-1.5 text-sm text-fg-muted">
        <div>
          <span className="text-fg-faint">Aktiver Backend:</span>{" "}
          <span className="font-medium text-fg">
            {activeBackend?.backend ?? "—"}
          </span>
          {activeBackend ? (
            <span className="text-fg-faint">
              {" "}
              ({activeBackend.description}, ~{activeBackend.expected_speedup}×
              CPU)
            </span>
          ) : null}
        </div>
        <div>
          <span className="text-fg-faint">CPU-Threads:</span>{" "}
          {hardware?.cpu_logical_cores ?? "—"}
        </div>
        <div>
          <span className="text-fg-faint">RAM:</span> {ramText}
        </div>
        <div>
          <span className="text-fg-faint">GPU:</span> {gpuFlags.join(", ")}
        </div>
        <div>
          <span className="text-fg-faint">Compute-Libs:</span>{" "}
          {libsFlags.join(", ")}
        </div>
        {llmRecommendation ? (
          <div>
            <span className="text-fg-faint">LLM-Empfehlung fuer dein RAM:</span>{" "}
            <span className="font-medium text-fg">
              {llmRecommendation.label}
            </span>
            {llmMismatch ? (
              <button
                type="button"
                onClick={() => onPickLlmSlot(llmRecommendation.slot)}
                className="ml-2 text-xs underline text-brand hover:text-brand-hover pointer-events-auto"
              >
                uebernehmen
              </button>
            ) : (
              <span className="ml-2 text-xs text-status-recording">
                ✓ aktiv
              </span>
            )}
          </div>
        ) : null}
        {showVariantHint ? (
          <div className="mt-1 text-xs rounded-md bg-brand/10 border border-brand/40 px-2.5 py-1.5 text-fg">
            Tip: Ein {hardware.recommended_variant}-Build koennte ~
            {hardware.recommended_speedup}× CPU geben (statt aktuell ~
            {activeBackend.expected_speedup}×). Phase-3-Bundle-Matrix wird das
            adressieren.
          </div>
        ) : null}
      </div>
    </Field>
  );
}
