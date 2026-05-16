// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { useModesStore } from "../store";
import {
  ipcDeleteMode,
  ipcGetSessionInfo,
  ipcStartRecording,
  type SessionInfo,
} from "../lib/tauri";
import type { Mode } from "../lib/types";
import ModeEditor from "../components/ModeEditor";

const primaryBtn =
  "inline-flex items-center px-3 py-2 rounded-md bg-brand text-brand-contrast text-sm font-medium hover:bg-brand-hover transition-colors disabled:bg-elevated disabled:text-fg-faint disabled:cursor-not-allowed";

export default function Modes(): JSX.Element {
  const modes = useModesStore((s) => s.modes);
  const loading = useModesStore((s) => s.loading);
  const error = useModesStore((s) => s.error);
  const load = useModesStore((s) => s.load);

  const [editing, setEditing] = useState<Mode | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [opError, setOpError] = useState<string | null>(null);
  const [session, setSession] = useState<SessionInfo | null>(null);
  const [triggering, setTriggering] = useState<string | null>(null);

  useEffect(() => {
    void ipcGetSessionInfo()
      .then(setSession)
      .catch(() => null);
  }, []);

  const onTrigger = async (modeId: string) => {
    setTriggering(modeId);
    setOpError(null);
    try {
      await ipcStartRecording(modeId);
    } catch (e) {
      setOpError(String(e));
    } finally {
      setTriggering(null);
    }
  };

  useEffect(() => {
    void load();
  }, [load]);

  const onDelete = async (id: string, name: string) => {
    if (!window.confirm(`Modus „${name}" wirklich loeschen?`)) {
      return;
    }
    try {
      await ipcDeleteMode(id);
      await load();
    } catch (e) {
      setOpError(String(e));
    }
  };

  const onSaved = async () => {
    setEditing(null);
    setShowNew(false);
    // Hot-Reload greift in ~200ms, aber expliziter Pull schadet nicht.
    await load();
  };

  if (loading) {
    return <div className="text-fg-faint">Lade Modi…</div>;
  }
  if (error) {
    return (
      <div className="rounded-md bg-status-error/10 border border-status-error/40 px-3 py-2 text-sm text-status-error">
        {error}
      </div>
    );
  }

  const noHotkeys = session !== null && !session.global_hotkeys_supported;

  return (
    <div className="flex flex-col gap-3">
      {noHotkeys ? (
        <div className="rounded-md bg-status-processing/10 border border-status-processing/40 px-3 py-2 text-xs text-status-processing">
          <strong>Display-Server: {session?.display_server}.</strong> Globale
          Hotkeys sind hier nicht verfuegbar (Phase 5 ergaenzt das via
          xdg-desktop-portal). Nutze die <em>Trigger</em>-Buttons unten — sie
          starten/stoppen die Aufnahme. Der Text landet im Clipboard;
          {session?.auto_paste_supported
            ? ""
            : " danach drueck Ctrl+V in der Ziel-App."}
        </div>
      ) : null}
      <div className="flex justify-between items-start gap-4">
        <p className="text-sm text-fg-muted max-w-3xl">
          Modi werden in{" "}
          <code className="text-brand font-mono">app_config_dir/modes/</code>{" "}
          als TOML-Dateien gespeichert. UI-Aenderungen schreiben dorthin; der
          Hot-Reload-Watcher pickt sie auf. Du kannst die Dateien auch direkt im
          Editor anfassen.
        </p>
        <button
          type="button"
          onClick={() => setShowNew(true)}
          className={`${primaryBtn} shrink-0`}
        >
          + Neuer Modus
        </button>
      </div>

      {opError ? (
        <div className="rounded-md bg-status-error/10 border border-status-error/40 px-3 py-2 text-sm text-status-error">
          {opError}
        </div>
      ) : null}

      <table className="w-full text-sm">
        <thead className="text-left text-fg-muted border-b border-outline">
          <tr>
            <th className="py-2 font-medium">Name</th>
            <th className="py-2 font-medium">STT</th>
            <th className="py-2 font-medium">Nachbearbeitung</th>
            <th className="py-2 font-medium">Inject</th>
            <th className="py-2 w-32"></th>
          </tr>
        </thead>
        <tbody>
          {modes.map((m) => (
            <tr
              key={m.id}
              className="border-b border-outline/60 hover:bg-elevated/60 transition-colors"
            >
              <td className="py-2">
                <div className="font-medium text-fg">{m.name}</div>
                <div className="text-xs text-fg-faint font-mono">
                  id: {m.id}
                </div>
              </td>
              <td className="py-2 capitalize text-fg-muted">
                {m.transcription}
                {m.cloud_stt_provider ? ` / ${m.cloud_stt_provider}` : ""}
              </td>
              <td className="py-2 capitalize text-fg-muted">
                {m.processing}
                {m.cloud_llm_provider ? ` / ${m.cloud_llm_provider}` : ""}
                {m.cloud_llm_model ? ` (${m.cloud_llm_model})` : ""}
              </td>
              <td className="py-2 capitalize text-fg-muted">
                {m.injection_method}
              </td>
              <td className="py-2 text-right">
                <button
                  type="button"
                  onClick={() => void onTrigger(m.id)}
                  disabled={triggering !== null}
                  className="text-xs px-2 py-1 rounded-md bg-brand text-brand-contrast hover:bg-brand-hover transition-colors mr-1 disabled:opacity-50"
                  title="Aufnahme starten/stoppen (Toggle)"
                >
                  {triggering === m.id ? "…" : "Trigger"}
                </button>
                <button
                  type="button"
                  onClick={() => setEditing(m)}
                  className="text-xs px-2 py-1 rounded-md bg-elevated text-fg hover:bg-outline-strong/40 transition-colors mr-1"
                >
                  Edit
                </button>
                <button
                  type="button"
                  onClick={() => void onDelete(m.id, m.name)}
                  className="text-xs px-2 py-1 rounded-md bg-elevated text-fg-muted hover:bg-status-error/15 hover:text-status-error transition-colors"
                >
                  X
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {editing ? (
        <ModeEditor
          initial={editing}
          onClose={() => setEditing(null)}
          onSaved={() => void onSaved()}
        />
      ) : null}

      {showNew ? (
        <ModeEditor
          initial={null}
          onClose={() => setShowNew(false)}
          onSaved={() => void onSaved()}
        />
      ) : null}
    </div>
  );
}
