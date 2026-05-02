// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import Field from "../components/Field";
import { useSettingsStore } from "../store";

export default function Settings(): JSX.Element {
  const settings = useSettingsStore((s) => s.settings);
  const loading = useSettingsStore((s) => s.loading);
  const error = useSettingsStore((s) => s.error);
  const audioDevices = useSettingsStore((s) => s.audioDevices);
  const load = useSettingsStore((s) => s.load);
  const loadAudioDevices = useSettingsStore((s) => s.loadAudioDevices);
  const update = useSettingsStore((s) => s.update);

  useEffect(() => {
    void load();
    void loadAudioDevices();
  }, [load, loadAudioDevices]);

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
    </div>
  );
}
