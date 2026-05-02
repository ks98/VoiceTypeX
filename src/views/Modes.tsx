// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect } from "react";
import { useModesStore } from "../store";

export default function Modes(): JSX.Element {
  const modes = useModesStore((s) => s.modes);
  const loading = useModesStore((s) => s.loading);
  const error = useModesStore((s) => s.error);
  const load = useModesStore((s) => s.load);

  useEffect(() => {
    void load();
  }, [load]);

  if (loading) {
    return <div className="text-slate-500">Lade Modi…</div>;
  }
  if (error) {
    return (
      <div className="rounded-md bg-red-900/30 border border-red-700 px-3 py-2 text-sm text-red-300">
        {error}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-slate-400">
        Phase 1 ist read-only. Modi werden direkt aus den TOML-Dateien in{" "}
        <code className="text-brand-500">app_config_dir/modes/</code> gelesen.
        Aenderungen am File-System werden via Hot-Reload ueber{" "}
        <code className="text-brand-500">notify</code> erkannt.
      </p>
      <table className="w-full text-sm">
        <thead className="text-left text-slate-400 border-b border-slate-800">
          <tr>
            <th className="py-2">Name</th>
            <th className="py-2">Hotkey</th>
            <th className="py-2">STT</th>
            <th className="py-2">Nachbearbeitung</th>
            <th className="py-2">Inject</th>
          </tr>
        </thead>
        <tbody>
          {modes.map((m) => (
            <tr
              key={m.id}
              className="border-b border-slate-900 hover:bg-slate-900/40"
            >
              <td className="py-2">
                <div className="font-medium text-slate-100">{m.name}</div>
                <div className="text-xs text-slate-500">id: {m.id}</div>
              </td>
              <td className="py-2 font-mono text-xs">{m.hotkey}</td>
              <td className="py-2 capitalize">
                {m.transcription}
                {m.cloud_stt_provider ? ` / ${m.cloud_stt_provider}` : ""}
              </td>
              <td className="py-2 capitalize">
                {m.processing}
                {m.cloud_llm_provider ? ` / ${m.cloud_llm_provider}` : ""}
                {m.cloud_llm_model ? ` (${m.cloud_llm_model})` : ""}
              </td>
              <td className="py-2 capitalize">{m.injection_method}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
