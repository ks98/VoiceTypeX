// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import {
  ipcDeleteProviderKey,
  ipcGetProviderStatus,
  ipcSetProviderKey,
  ipcTestProviderConnection,
  type ProviderStatus,
} from "../lib/tauri";

type TestState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "ok" }
  | { kind: "error"; message: string };

const PROVIDER_LABELS: Record<string, string> = {
  xai: "xAI (Grok + STT, ein Schluessel)",
  openai: "OpenAI (GPT + Whisper)",
  anthropic: "Anthropic (Claude)",
  groq: "Groq (Whisper)",
  deepgram: "Deepgram (STT)",
};

export default function ApiKeysSection(): JSX.Element {
  const [status, setStatus] = useState<ProviderStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingProvider, setEditingProvider] = useState<string | null>(null);
  const [draftKey, setDraftKey] = useState("");
  const [saveError, setSaveError] = useState<string | null>(null);
  const [testStates, setTestStates] = useState<Record<string, TestState>>({});

  const refresh = async () => {
    setLoading(true);
    try {
      const fresh = await ipcGetProviderStatus();
      setStatus(fresh);
    } catch (e) {
      console.error("get_provider_status:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const onSave = async (provider: string) => {
    setSaveError(null);
    try {
      await ipcSetProviderKey(provider, draftKey);
      setEditingProvider(null);
      setDraftKey("");
      await refresh();
    } catch (e) {
      setSaveError(String(e));
    }
  };

  const onDelete = async (provider: string) => {
    setSaveError(null);
    try {
      await ipcDeleteProviderKey(provider);
      await refresh();
    } catch (e) {
      setSaveError(String(e));
    }
  };

  const onTest = async (provider: string) => {
    setTestStates((prev) => ({ ...prev, [provider]: { kind: "running" } }));
    try {
      await ipcTestProviderConnection(provider);
      setTestStates((prev) => ({ ...prev, [provider]: { kind: "ok" } }));
    } catch (e) {
      setTestStates((prev) => ({
        ...prev,
        [provider]: { kind: "error", message: String(e) },
      }));
    }
  };

  if (loading) {
    return <div className="text-slate-500">Lade Provider-Status…</div>;
  }

  return (
    <div className="flex flex-col gap-3">
      <div>
        <h2 className="text-lg font-semibold text-slate-200">
          Cloud-API-Keys (BYOK)
        </h2>
        <p className="text-xs text-slate-500 mt-1">
          Keys werden im OS-Keychain gespeichert, nie im Klartext auf Disk oder
          in Logs. Die UI sieht nur, ob ein Key gesetzt ist.
        </p>
      </div>

      {saveError ? (
        <div className="rounded-md bg-red-900/30 border border-red-700 px-3 py-2 text-sm text-red-300">
          {saveError}
        </div>
      ) : null}

      <div className="flex flex-col gap-2">
        {status.map((s) => {
          const test = testStates[s.provider] ?? { kind: "idle" };
          return (
            <div
              key={s.provider}
              className="flex flex-col gap-2 border border-slate-800 rounded p-3"
            >
              <div className="flex items-center gap-3">
                <div className="flex-1">
                  <div className="text-sm font-medium text-slate-100">
                    {PROVIDER_LABELS[s.provider] ?? s.provider}
                  </div>
                  <div className="text-xs text-slate-500">
                    {s.error ? (
                      <span className="text-red-400" title={s.error}>
                        ⚠ Keychain-Fehler:{" "}
                        {s.error.length > 60
                          ? s.error.slice(0, 60) + "…"
                          : s.error}
                      </span>
                    ) : s.configured ? (
                      <span className="text-emerald-400">✓ gesetzt</span>
                    ) : (
                      <span className="text-slate-500">nicht gesetzt</span>
                    )}
                  </div>
                </div>
                {editingProvider === s.provider ? (
                  <>
                    <input
                      type="password"
                      value={draftKey}
                      onChange={(e) => setDraftKey(e.target.value)}
                      placeholder="API-Key"
                      className="bg-slate-900 border border-slate-700 rounded px-2 py-1 text-xs w-64"
                      autoFocus
                    />
                    <button
                      type="button"
                      onClick={() => void onSave(s.provider)}
                      className="text-xs px-3 py-1 rounded bg-brand-700 hover:bg-brand-500"
                    >
                      Speichern
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setEditingProvider(null);
                        setDraftKey("");
                      }}
                      className="text-xs px-3 py-1 rounded bg-slate-800 hover:bg-slate-700"
                    >
                      Abbrechen
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      onClick={() => {
                        setEditingProvider(s.provider);
                        setDraftKey("");
                      }}
                      className="text-xs px-3 py-1 rounded bg-slate-800 hover:bg-slate-700"
                    >
                      {s.configured ? "Aendern" : "Setzen"}
                    </button>
                    {s.configured ? (
                      <>
                        <button
                          type="button"
                          onClick={() => void onTest(s.provider)}
                          disabled={test.kind === "running"}
                          className="text-xs px-3 py-1 rounded bg-slate-800 hover:bg-slate-700 disabled:opacity-50"
                        >
                          {test.kind === "running"
                            ? "Teste…"
                            : "Verbindung testen"}
                        </button>
                        <button
                          type="button"
                          onClick={() => void onDelete(s.provider)}
                          className="text-xs px-3 py-1 rounded bg-slate-800 hover:bg-red-900/40 hover:border-red-700 border border-transparent"
                        >
                          Loeschen
                        </button>
                      </>
                    ) : null}
                  </>
                )}
              </div>
              {test.kind === "ok" ? (
                <div className="text-xs text-emerald-400">
                  ✓ Verbindung erfolgreich
                </div>
              ) : null}
              {test.kind === "error" ? (
                <div className="text-xs text-red-400">{test.message}</div>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
