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
    return <Loading label="Lade Einstellungen…" />;
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
    <div className="flex gap-6 items-start">
      <SettingsSubNav />
      <div className="flex-1 flex flex-col gap-8 max-w-2xl min-w-0">
        {error ? <Banner tone="error">{error}</Banner> : null}

      <section
        id="hardware"
        className="scroll-mt-6 flex flex-col gap-4"
        aria-labelledby="hardware-heading"
      >
        <SectionHeader id="hardware-heading" title="Hardware" />
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
        <SectionHeader id="audio-hotkey-heading" title="Audio & Hotkey" />
      <Field
        label="Audio-Eingabegerät"
        hint="Leer = OS-Standard. Änderungen wirken beim nächsten Recording."
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
          title="Lokale Modelle (STT + LLM)"
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
        label="Lokales LLM-Modell (Embedded, Default)"
        hint="GGUF-Modell für den eingebauten llama-cpp-2-Pfad. Läuft ohne externen Daemon im VoiceTypeX-Prozess. Standard für alle Modi mit processing=local."
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

      <OllamaAdvancedSection
        url={settings.ollama_url}
        keepAlive={settings.ollama_keep_alive}
        onUrlChange={(v) => void update({ ollama_url: v })}
        onKeepAliveChange={(v) => void update({ ollama_keep_alive: v })}
        inputCls={inputCls}
      />

      </section>

      <section
        id="privacy-startup"
        className="scroll-mt-6 flex flex-col gap-4"
        aria-labelledby="privacy-startup-heading"
      >
        <SectionHeader
          id="privacy-startup-heading"
          title="Datenschutz & Start"
        />
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

      </section>

      <section
        id="cache"
        className="scroll-mt-6 flex flex-col gap-4"
        aria-labelledby="cache-heading"
      >
        <SectionHeader id="cache-heading" title="Cache & Modelle" />
        <CacheManagementField />
      </section>

      <section
        id="diagnostics"
        className="scroll-mt-6 flex flex-col gap-4"
        aria-labelledby="diagnostics-heading"
      >
        <SectionHeader
          id="diagnostics-heading"
          title="Diagnose & Tests"
        />
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
 * Sticky-Sub-Nav fuer die Settings-Page. Auf kleinen Fenstern (<1024 px)
 * ausgeblendet — der User scrollt dann linear durch. Auf groesseren
 * Fenstern bleibt sie wie ein Outline-Index sichtbar.
 *
 * Kein Scroll-Spy: das waere ein zusaetzlicher IntersectionObserver,
 * lohnt sich nicht fuer 8 Sektionen. Anchor-Links reichen.
 */
function SettingsSubNav(): JSX.Element {
  const items: Array<{ id: string; label: string }> = [
    { id: "hardware", label: "Hardware" },
    { id: "audio-hotkey", label: "Audio & Hotkey" },
    { id: "local-models", label: "Lokale Modelle" },
    { id: "privacy-startup", label: "Datenschutz & Start" },
    { id: "cache", label: "Cache" },
    { id: "diagnostics", label: "Diagnose & Tests" },
    { id: "api-keys", label: "Cloud-API-Keys" },
    { id: "danger-zone", label: "Gefahrenzone" },
  ];
  return (
    <nav
      aria-label="Settings-Bereiche"
      className="hidden lg:flex flex-col gap-0.5 w-44 shrink-0 sticky top-0 self-start"
    >
      {items.map((it) => (
        <a
          key={it.id}
          href={`#${it.id}`}
          className="px-3 py-1.5 rounded-md text-sm text-fg-muted hover:text-fg hover:bg-elevated transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40"
        >
          {it.label}
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
 * Ollama-Konfiguration als collapsed Advanced-Block.
 *
 * Begründung: Embedded ist seit dem Default-Switch der Standardpfad —
 * 95 % der User berühren Ollama nie. Die Konfiguration komplett aus dem
 * UI zu nehmen wäre aber zu aggressiv (Power-User mit eigener Daemon-
 * Installation brauchen sie). Daher: collapsible `<details>`, klar als
 * „nur für Ollama-Benutzer" markiert, mit beiden Settings (`ollama_url`
 * und `ollama_keep_alive`) zusammen — vorher fehlte Keep-Alive komplett
 * im UI.
 */
function OllamaAdvancedSection({
  url,
  keepAlive,
  onUrlChange,
  onKeepAliveChange,
  inputCls,
}: {
  url: string;
  keepAlive: string;
  onUrlChange: (v: string) => void;
  onKeepAliveChange: (v: string) => void;
  inputCls: string;
}): JSX.Element {
  return (
    <details className="rounded-md border border-outline/60 bg-surface/40 p-3">
      <summary className="text-sm font-medium text-fg-muted cursor-pointer hover:text-fg transition-colors select-none">
        Ollama-Konfiguration (optional, für externe Daemon-Nutzung)
      </summary>
      <div className="flex flex-col gap-4 pt-3 mt-2 border-t border-outline/40">
        <p className="text-xs text-fg-faint">
          VoiceTypeX nutzt standardmäßig den eingebauten Embedded-LLM-Pfad
          (kein externer Daemon nötig). Diese Einstellungen sind nur
          relevant, wenn ein Modus <code className="text-fg-muted font-mono">local_engine = "ollama"</code>{" "}
          setzt — z.B. wenn du dein eigenes Ollama-Setup mit Custom-Port
          oder einem Modell betreiben willst, das nicht als GGUF-Slot
          verfügbar ist.
        </p>
        <Field
          label="Ollama-Endpunkt"
          hint="HTTP-URL der Ollama-Daemon. Standardport ist 11434."
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
          label="Ollama Keep-Alive"
          hint='Wie lange Ollama das Modell nach dem letzten Call im RAM/VRAM hält. Duration-String: "5m" (Default), "0" für sofortiges Unload, "-1" für unbegrenzt warm.'
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

/**
 * Re-Trigger fuer den Onboarding-Wizard. Setzt `onboarding_done: false` —
 * App.tsx rendert den Wizard, sobald das Settings-Update durchgelaufen ist.
 * Mit Confirm-Dialog, weil der Wizard das Hauptfenster temporaer komplett
 * uebernimmt.
 */
function SetupAssistantRetriggerField({
  onTrigger,
}: {
  onTrigger: () => void;
}): JSX.Element {
  const handler = () => {
    if (
      window.confirm(
        "Setup-Assistent erneut starten?\n\nDer Assistent oeffnet sich anstelle dieses Fensters und fragt erneut nach Whisper-Modell, API-Key und LLM-Modell. Bestehende Einstellungen bleiben unveraendert.",
      )
    ) {
      onTrigger();
    }
  };
  return (
    <Field
      label="Setup-Assistent"
      hint="Wenn sich deine Hardware geändert hat oder du den Assistenten erneut durchlaufen möchtest."
    >
      <Button variant="secondary" onClick={handler} className="self-start">
        Setup-Assistent erneut starten
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
        <Loading label="Lade Hardware-Info…" inline />
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

/**
 * Cache-Management-Field: listet alle heruntergeladenen Modell-Files,
 * erlaubt einzelnes oder bulk-Loeschen. Pflegt keine Settings, Modes
 * oder Secrets — die haben ihre eigenen User-Daten-Reset-Flows.
 */
function CacheManagementField(): JSX.Element {
  const [files, setFiles] = useState<CachedFile[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const list = await ipcListCachedFiles();
      setFiles(list);
    } catch (e) {
      setStatus(`Liste laden fehlgeschlagen: ${e}`);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const fmtSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} kB`;
    if (bytes < 1024 * 1024 * 1024)
      return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  };

  const kindLabel = (kind: CachedFile["kind"]): string => {
    switch (kind) {
      case "whisper":
        return "Whisper";
      case "vad":
        return "VAD";
      case "llm":
        return "LLM";
      case "partial":
        return "Abgebrochen";
      case "other":
        return "Sonstiges";
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
    if (!confirm(`"${filename}" wirklich loeschen?`)) return;
    setBusy(true);
    setStatus(null);
    try {
      const freed = await ipcDeleteCachedFile(filename);
      setStatus(`Geloescht (${fmtSize(freed)} freigegeben).`);
      await refresh();
    } catch (e) {
      setStatus(`Loeschen fehlgeschlagen: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onCleanPartials = async () => {
    setBusy(true);
    setStatus(null);
    try {
      const freed = await ipcCleanPartialDownloads();
      setStatus(`Partial-Downloads aufgeraeumt (${fmtSize(freed)} freigegeben).`);
      await refresh();
    } catch (e) {
      setStatus(`Aufraeumen fehlgeschlagen: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const onDeleteAll = async () => {
    if (
      !confirm(
        "Alle heruntergeladenen Modelle loeschen? " +
          "(Whisper, VAD, LLM, Partials — werden beim naechsten Bedarf " +
          "neu heruntergeladen)",
      )
    ) {
      return;
    }
    setBusy(true);
    setStatus(null);
    try {
      const freed = await ipcDeleteAllModels();
      setStatus(`Alle Modelle geloescht (${fmtSize(freed)} freigegeben).`);
      await refresh();
    } catch (e) {
      setStatus(`Loeschen fehlgeschlagen: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const totalBytes = files?.reduce((sum, f) => sum + f.size_bytes, 0) ?? 0;

  return (
    <Field
      label="Cache verwalten"
      hint="Heruntergeladene Modell-Files. Einzeln oder gebuendelt loeschen. Settings, Modi und API-Keys sind nicht betroffen."
    >
      <div className="flex flex-col gap-3">
        {files === null ? (
          <Loading label="Lade Datei-Liste…" inline />
        ) : files.length === 0 ? (
          <div className="text-sm text-fg-faint">
            Keine Files im Modell-Cache (yet — werden beim ersten Download
            angelegt).
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
                  {fmtSize(f.size_bytes)}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => void onDeleteSingle(f.filename)}
                  disabled={busy}
                >
                  Loeschen
                </Button>
              </div>
            ))}
            <div className="text-xs text-fg-faint pt-1">
              Gesamt: {files.length} File{files.length === 1 ? "" : "s"} ·{" "}
              {fmtSize(totalBytes)}
            </div>
          </div>
        )}

        <div className="flex flex-wrap gap-2">
          <Button
            variant="secondary"
            onClick={() => void onCleanPartials()}
            disabled={busy}
          >
            Abgebrochene Downloads aufraeumen
          </Button>
          <Button
            variant="secondary"
            onClick={() => void onDeleteAll()}
            disabled={busy}
          >
            Alle Modelle loeschen
          </Button>
          <Button variant="ghost" onClick={() => void refresh()} disabled={busy}>
            Aktualisieren
          </Button>
        </div>

        {status ? (
          <div className="text-xs text-fg-muted">{status}</div>
        ) : null}
      </div>
    </Field>
  );
}

/**
 * Gefahrenzone — Reset- und Uninstall-Vorbereitungs-Aktionen. Bewusst
 * am Ende der Seite und visuell abgesetzt (rote Outline), damit sie
 * nicht versehentlich angeklickt werden. Jeder Button hat einen
 * Bestaetigungs-Dialog.
 */
function DangerZoneSection(): JSX.Element {
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
      "Alle Cloud-API-Keys (xAI, OpenAI, Anthropic, Groq, Deepgram) aus File-Storage UND OS-Keychain löschen?\n\nDanach musst du die Keys neu eingeben, um Cloud-Provider zu nutzen.",
      ipcResetApiKeys,
      "Alle API-Keys geloescht.",
    );

  const onResetWayland = () =>
    run(
      "wayland",
      "Wayland-Permission-Token löschen?\n\nBeim nächsten Auto-Paste-Inject zeigt der Compositor wieder den Berechtigungs-Dialog. Auf X11/Windows hat das keinen Effekt.",
      ipcResetWaylandToken,
      "Wayland-Token geloescht.",
    );

  const onFactoryReset = () =>
    run(
      "factory",
      "App vollständig zurücksetzen?\n\n• Settings → Defaults\n• Modi → 6 mitgelieferte Defaults (eigene gehen verloren)\n• API-Keys → gelöscht\n• Wayland-Token → gelöscht\n\nHeruntergeladene Modelle bleiben erhalten — die musst du separat über das Cache-Management löschen.\n\nDieser Schritt ist NICHT rückgängig zu machen.",
      ipcResetAppFactory,
      "App auf Werkseinstellungen zurückgesetzt. Empfehlung: App neu starten, damit alle In-Memory-States frisch geladen sind.",
    );

  return (
    <Field label="Gefahrenzone" hint="Aktionen, die User-Daten dauerhaft entfernen. Jede Aktion fragt vor Ausführung nach.">
      <div className="rounded-md border border-status-error/40 bg-status-error/5 p-3 flex flex-col gap-3">
        <DangerRow
          title="Cloud-API-Keys löschen"
          description="Entfernt alle Provider-Keys aus secrets.json und OS-Keychain."
          buttonLabel={busy === "keys" ? "Lösche…" : "API-Keys löschen"}
          onClick={() => void onResetApiKeys()}
          disabled={busy !== null}
        />
        <DangerRow
          title="Wayland-Permission-Token löschen"
          description="Forciert beim nächsten Auto-Paste wieder den xdg-portal-Dialog. Auf X11/Windows No-Op."
          buttonLabel={busy === "wayland" ? "Lösche…" : "Token löschen"}
          onClick={() => void onResetWayland()}
          disabled={busy !== null}
        />
        <DangerRow
          title="App vollständig zurücksetzen"
          description="Settings, Modi, API-Keys und Wayland-Token raus. Modelle bleiben erhalten."
          buttonLabel={busy === "factory" ? "Setze zurück…" : "Werkseinstellungen"}
          onClick={() => void onFactoryReset()}
          disabled={busy !== null}
          severe
        />
        {status ? (
          <div
            className={`text-xs ${
              status.kind === "ok"
                ? "text-status-done"
                : "text-status-error"
            }`}
          >
            {status.text}
          </div>
        ) : null}
        <div className="text-xs text-fg-faint">
          Für eine vollständige Deinstallation siehe{" "}
          <code className="text-fg-muted">scripts/uninstall-cleanup.sh</code>
          {" "}im Repo oder den Abschnitt „Deinstallation" in der README.
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
