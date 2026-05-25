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
import Button from "../components/Button";
import Banner from "../components/Banner";
import Loading from "../components/Loading";
import { useT } from "../i18n";

export default function Modes(): JSX.Element {
  const t = useT();
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
    if (!window.confirm(t("modes.confirm_delete", { name }))) {
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
    // Hot-reload kicks in in ~200ms, but an explicit pull doesn't hurt.
    await load();
  };

  if (loading) {
    return <Loading label={t("modes.loading")} />;
  }
  if (error) {
    return <Banner tone="error">{error}</Banner>;
  }

  const noHotkeys = session !== null && !session.global_hotkeys_supported;
  const triggerLabel = t("modes.no_hotkeys.action_emphasis");
  const noHotkeyServer = session?.display_server ?? "";

  return (
    <div className="flex flex-col gap-3">
      {noHotkeys ? (
        <Banner tone="warning">
          <strong>
            {t("modes.no_hotkeys.server_label", { server: noHotkeyServer })}
          </strong>{" "}
          {t("modes.no_hotkeys.body_prefix")}{" "}
          <em>{triggerLabel}</em>
          {session?.auto_paste_supported
            ? t("modes.no_hotkeys.body_suffix_paste")
            : t("modes.no_hotkeys.body_suffix_manual")}
        </Banner>
      ) : null}
      <div className="flex justify-between items-start gap-4">
        <p className="text-sm text-fg-muted max-w-3xl">
          {t("modes.intro_prefix")}{" "}
          <code className="text-brand font-mono">app_config_dir/modes/</code>{" "}
          {t("modes.intro_suffix")}
        </p>
        <Button onClick={() => setShowNew(true)} className="shrink-0">
          {t("modes.new_button")}
        </Button>
      </div>

      {opError ? <Banner tone="error">{opError}</Banner> : null}

      <table className="w-full text-sm">
        <thead className="text-left text-fg-muted border-b border-outline">
          <tr>
            <th scope="col" className="py-2 font-medium">
              {t("modes.table.name")}
            </th>
            <th scope="col" className="py-2 font-medium">
              {t("modes.table.stt")}
            </th>
            <th scope="col" className="py-2 font-medium">
              {t("modes.table.processing")}
            </th>
            <th scope="col" className="py-2 font-medium">
              {t("modes.table.inject")}
            </th>
            <th scope="col" className="py-2 w-44">
              <span className="sr-only">{t("modes.table.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {modes.map((m) => (
            <tr
              key={m.id}
              className="group border-b border-outline/60 hover:bg-elevated/60 transition-colors"
            >
              <td className="py-2">
                <div className="font-medium text-fg">{m.name}</div>
                {/* ID visible only on hover — the power user needs
                    it for filename mapping, the normal user does
                    not. */}
                <div className="text-xs text-fg-faint font-mono opacity-0 group-hover:opacity-100 transition-opacity">
                  {t("modes.row.id_label", { id: m.id })}
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
              <td className="py-2">
                {/* Trigger isolated on the left, Edit + Delete on
                    the right with a large gap. Click-trap avoided:
                    Delete is not directly next to the frequently
                    used Trigger. */}
                <div className="flex justify-end items-center gap-2">
                  <Button
                    size="sm"
                    onClick={() => void onTrigger(m.id)}
                    disabled={triggering !== null}
                    title={t("modes.row.trigger_tooltip")}
                  >
                    {triggering === m.id
                      ? t("modes.row.trigger_busy")
                      : t("modes.row.trigger")}
                  </Button>
                  <span className="w-2" aria-hidden />
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => setEditing(m)}
                  >
                    {t("modes.row.edit")}
                  </Button>
                  <Button
                    size="sm"
                    variant="danger"
                    onClick={() => void onDelete(m.id, m.name)}
                    aria-label={t("modes.row.delete_aria", { name: m.name })}
                    title={t("modes.row.delete_aria", { name: m.name })}
                  >
                    <TrashIcon />
                  </Button>
                </div>
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

function TrashIcon(): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      className="h-3.5 w-3.5"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6h14zM10 11v6M14 11v6" />
    </svg>
  );
}
