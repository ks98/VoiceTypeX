// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import Button from "./Button";
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

const inputCls =
  "bg-surface border border-outline rounded-md px-2 py-1.5 text-xs text-fg placeholder:text-fg-faint focus:outline-none focus:border-brand focus:ring-1 focus:ring-brand/40";

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
    return <div className="text-fg-faint">Lade Provider-Status…</div>;
  }

  return (
    <div className="flex flex-col gap-3">
      <div>
        <h2 className="text-lg font-semibold text-fg">
          Cloud-API-Keys (BYOK)
        </h2>
        <p className="text-xs text-fg-faint mt-1">
          Keys werden im OS-Keychain gespeichert, nie im Klartext auf Disk oder
          in Logs. Die UI sieht nur, ob ein Key gesetzt ist.
        </p>
      </div>

      {saveError ? (
        <div className="rounded-md bg-status-error/10 border border-status-error/40 px-3 py-2 text-sm text-status-error">
          {saveError}
        </div>
      ) : null}

      <div className="flex flex-col gap-2">
        {status.map((s) => {
          const test = testStates[s.provider] ?? { kind: "idle" };
          return (
            <div
              key={s.provider}
              className="flex flex-col gap-2 border border-outline rounded-md p-3 bg-surface"
            >
              <div className="flex items-center gap-3 flex-wrap">
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-fg">
                    {PROVIDER_LABELS[s.provider] ?? s.provider}
                  </div>
                  <div className="text-xs">
                    {s.error ? (
                      <span className="text-status-error" title={s.error}>
                        ⚠ Keychain-Fehler:{" "}
                        {s.error.length > 60
                          ? s.error.slice(0, 60) + "…"
                          : s.error}
                      </span>
                    ) : s.configured ? (
                      <span className="text-status-done">✓ gesetzt</span>
                    ) : (
                      <span className="text-fg-faint">nicht gesetzt</span>
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
                      className={`${inputCls} w-64`}
                      autoFocus
                    />
                    <Button size="sm" onClick={() => void onSave(s.provider)}>
                      Speichern
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        setEditingProvider(null);
                        setDraftKey("");
                      }}
                    >
                      Abbrechen
                    </Button>
                  </>
                ) : (
                  <>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        setEditingProvider(s.provider);
                        setDraftKey("");
                      }}
                    >
                      {s.configured ? "Aendern" : "Setzen"}
                    </Button>
                    {s.configured ? (
                      <>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => void onTest(s.provider)}
                          disabled={test.kind === "running"}
                        >
                          {test.kind === "running"
                            ? "Teste…"
                            : "Verbindung testen"}
                        </Button>
                        <Button
                          size="sm"
                          variant="danger"
                          onClick={() => void onDelete(s.provider)}
                        >
                          Loeschen
                        </Button>
                      </>
                    ) : null}
                  </>
                )}
              </div>
              {test.kind === "ok" ? (
                <div className="text-xs text-status-done">
                  ✓ Verbindung erfolgreich
                </div>
              ) : null}
              {test.kind === "error" ? (
                <div className="text-xs text-status-error">{test.message}</div>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
