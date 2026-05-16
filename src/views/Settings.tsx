// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import Field from "../components/Field";
import ApiKeysSection from "../components/ApiKeysSection";
import TestTranscriptionSection from "../components/TestTranscriptionSection";
import AutoPasteTestSection from "../components/AutoPasteTestSection";
import { useSettingsStore } from "../store";
import {
  ipcDownloadDefaultModel,
  ipcGetEffectiveMenuHotkey,
  ipcGetSessionInfo,
  type ModelDownloadProgress,
  type SessionInfo,
} from "../lib/tauri";

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

  useEffect(() => {
    void load();
    void loadAudioDevices();
    void ipcGetSessionInfo()
      .then(setSession)
      .catch(() => null);
    void ipcGetEffectiveMenuHotkey()
      .then(setEffectiveHotkey)
      .catch(() => null);
  }, [load, loadAudioDevices]);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    void listen<ModelDownloadProgress>("model-download-progress", (event) =>
      setProgress(event.payload),
    ).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  if (loading || !settings) {
    return <div className="text-slate-500">Lade Einstellungen…</div>;
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
        <div className="rounded-md bg-red-900/30 border border-red-700 px-3 py-2 text-sm text-red-300">
          {error}
        </div>
      ) : null}

      <Field
        label="Audio-Eingabegeraet"
        hint="Leer = OS-Standard. Aenderungen wirken beim naechsten Recording."
      >
        <select
          className="bg-slate-900 border border-slate-700 rounded px-3 py-2 text-sm"
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

      <Field
        label="Lokales Whisper-Modell"
        hint="Default-Slot wird beim ersten Start nach app_data_dir/models/ heruntergeladen. Eigener Pfad ueberschreibt das."
      >
        <div className="flex gap-2">
          <input
            type="text"
            className="flex-1 bg-slate-900 border border-slate-700 rounded px-3 py-2 text-sm"
            placeholder="(Default-Modell aus Slot)"
            value={settings.whisper_model_path ?? ""}
            onChange={(e) =>
              void update({ whisper_model_path: e.target.value || null })
            }
          />
          <button
            type="button"
            onClick={() => void onPickModel()}
            className="px-3 py-2 rounded bg-slate-800 hover:bg-slate-700 text-sm"
          >
            Datei waehlen…
          </button>
        </div>
        <select
          className="bg-slate-900 border border-slate-700 rounded px-3 py-2 text-sm"
          value={settings.whisper_default_slot}
          onChange={(e) =>
            void update({ whisper_default_slot: e.target.value })
          }
        >
          <option value="large-v3-turbo-q5_0">
            large-v3-turbo-q5_0 (~547 MB, empfohlen)
          </option>
          <option value="small-q5_1">small-q5_1 (~181 MB, sparsam)</option>
          <option value="large-v3-turbo">
            large-v3-turbo (unquantisiert, ~1.6 GB)
          </option>
        </select>
        <div className="flex flex-col gap-1.5">
          <button
            type="button"
            onClick={() => void onDownloadDefault()}
            disabled={downloading}
            className="self-start px-3 py-2 rounded bg-brand-700 hover:bg-brand-500 disabled:bg-slate-800 disabled:text-slate-500 text-sm"
          >
            {downloading
              ? "Lade Modell…"
              : "Default-Modell jetzt herunterladen"}
          </button>
          {progress ? (
            <div className="flex flex-col gap-1 text-xs text-slate-400">
              <div>
                {fmtMb(progress.downloaded)}
                {progress.total ? ` von ${fmtMb(progress.total)}` : ""}
                {progressPct !== null ? ` (${progressPct} %)` : ""}
              </div>
              {progressPct !== null ? (
                <div className="h-1.5 bg-slate-800 rounded overflow-hidden">
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
          className="bg-slate-900 border border-slate-700 rounded px-3 py-2 text-sm w-32"
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
        hint="Lokales LLM. Standardport von Ollama ist 11434."
      >
        <input
          type="text"
          className="bg-slate-900 border border-slate-700 rounded px-3 py-2 text-sm"
          value={settings.ollama_url}
          onChange={(e) => void update({ ollama_url: e.target.value })}
        />
      </Field>

      <Field
        label="Diagnose-Logging"
        hint="Erlaubt Audio-Metadata, Transkripte und LLM-Antworten in den Logs. Default OFF (Datenschutz)."
      >
        <label className="flex items-center gap-2 text-sm">
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
        <label className="flex items-center gap-2 text-sm">
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

/**
 * Menue-Hotkey-Anzeige + Bearbeitung. Plattform-abhaengig:
 *
 * - **Wayland**: read-only. KDE/GNOME verwalten die Tastenbindung selbst;
 *   ein Aendern des Settings-Werts schlaegt nur als *Vorschlag* fuer den
 *   allerersten Start durch. Wir zeigen den vom Compositor effektiv
 *   gebundenen Trigger an und verweisen auf System Settings.
 * - **X11 / Windows**: editierbar. Die App registriert den Hotkey direkt
 *   ueber `tauri-plugin-global-shortcut`, der Settings-Wert ist die
 *   Wahrheit.
 */
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
        <div className="bg-slate-900/60 border border-slate-700 rounded px-3 py-2 text-sm font-mono w-72 text-slate-300">
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
        className="bg-slate-900 border border-slate-700 rounded px-3 py-2 text-sm font-mono w-72"
        value={settingsValue}
        onChange={(e) => onChange(e.target.value)}
        placeholder="CommandOrControl+Alt+Space"
      />
    </Field>
  );
}
