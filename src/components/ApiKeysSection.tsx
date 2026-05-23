// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import Banner from "./Banner";
import Button from "./Button";
import Input from "./Input";
import Loading from "./Loading";
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

type SaveState =
  | { kind: "idle" }
  | { kind: "saving" }
  | { kind: "testing" };

const PROVIDER_LABELS: Record<string, string> = {
  xai: "xAI (Grok + STT, ein Schlüssel)",
  openai: "OpenAI (GPT + Whisper)",
  anthropic: "Anthropic (Claude)",
  groq: "Groq (Whisper)",
  deepgram: "Deepgram (STT)",
};

function formatRelative(from: number, now: number): string {
  const sec = Math.max(0, Math.round((now - from) / 1000));
  if (sec < 60) return "getestet gerade eben";
  const min = Math.round(sec / 60);
  if (min < 60) return `getestet vor ${min} min`;
  const hr = Math.round(min / 60);
  if (hr < 24) return `getestet vor ${hr} h`;
  const days = Math.round(hr / 24);
  return `getestet vor ${days} d`;
}

function EyeIcon({ open }: { open: boolean }): JSX.Element {
  // Inline-SVG vermeidet eine Icon-Lib-Abhaengigkeit fuer einen Single-Use.
  if (open) {
    return (
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
    );
  }
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M17.94 17.94A10.94 10.94 0 0 1 12 19c-6.5 0-10-7-10-7a18.5 18.5 0 0 1 4.06-5.06" />
      <path d="M9.9 4.24A10.94 10.94 0 0 1 12 4c6.5 0 10 7 10 7a18.6 18.6 0 0 1-2.16 3.19" />
      <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24" />
      <line x1="2" y1="2" x2="22" y2="22" />
    </svg>
  );
}

export default function ApiKeysSection(): JSX.Element {
  const [status, setStatus] = useState<ProviderStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [editingProvider, setEditingProvider] = useState<string | null>(null);
  const [draftKey, setDraftKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saveState, setSaveState] = useState<SaveState>({ kind: "idle" });
  const [testStates, setTestStates] = useState<Record<string, TestState>>({});
  const [lastTested, setLastTested] = useState<Record<string, number>>({});
  const [now, setNow] = useState(() => Date.now());

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

  // Relative-Time-Tick fuer "getestet vor N min". 60s reicht, da die kleinste
  // Aufloesung Minuten ist.
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, []);

  const runTest = async (provider: string): Promise<void> => {
    setTestStates((prev) => ({ ...prev, [provider]: { kind: "running" } }));
    try {
      await ipcTestProviderConnection(provider);
      setTestStates((prev) => ({ ...prev, [provider]: { kind: "ok" } }));
      setLastTested((prev) => ({ ...prev, [provider]: Date.now() }));
    } catch (e) {
      setTestStates((prev) => ({
        ...prev,
        [provider]: { kind: "error", message: String(e) },
      }));
    }
  };

  const onSave = async (provider: string) => {
    setSaveError(null);
    setSaveState({ kind: "saving" });
    try {
      await ipcSetProviderKey(provider, draftKey);
      setEditingProvider(null);
      setDraftKey("");
      setShowKey(false);
      await refresh();
      setSaveState({ kind: "testing" });
      await runTest(provider);
    } catch (e) {
      setSaveError(String(e));
    } finally {
      setSaveState({ kind: "idle" });
    }
  };

  const onDelete = async (provider: string) => {
    setSaveError(null);
    try {
      await ipcDeleteProviderKey(provider);
      setTestStates((prev) => {
        const next = { ...prev };
        delete next[provider];
        return next;
      });
      setLastTested((prev) => {
        const next = { ...prev };
        delete next[provider];
        return next;
      });
      await refresh();
    } catch (e) {
      setSaveError(String(e));
    }
  };

  const onTest = (provider: string) => void runTest(provider);

  if (loading) {
    return <Loading label="Lade Provider-Status…" />;
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

      {saveError ? <Banner tone="error">{saveError}</Banner> : null}

      <div className="flex flex-col gap-2">
        {status.map((s) => {
          const test = testStates[s.provider] ?? { kind: "idle" };
          const tested = lastTested[s.provider];
          const isEditing = editingProvider === s.provider;
          const saveLabel =
            saveState.kind === "saving"
              ? "Speichere…"
              : saveState.kind === "testing"
                ? "Teste…"
                : "Speichern";
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
                {isEditing ? (
                  <>
                    <div className="relative w-64">
                      <Input
                        density="compact"
                        type={showKey ? "text" : "password"}
                        value={draftKey}
                        onChange={(e) => setDraftKey(e.target.value)}
                        placeholder="API-Key"
                        className="pr-8"
                        autoFocus
                      />
                      <button
                        type="button"
                        onClick={() => setShowKey((v) => !v)}
                        aria-label={
                          showKey
                            ? "Schlüssel verbergen"
                            : "Schlüssel anzeigen"
                        }
                        aria-pressed={showKey}
                        className="absolute right-1 top-1/2 -translate-y-1/2 inline-flex items-center justify-center h-6 w-6 rounded text-fg-muted hover:text-fg hover:bg-elevated focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40"
                      >
                        <EyeIcon open={showKey} />
                      </button>
                    </div>
                    <Button
                      size="sm"
                      onClick={() => void onSave(s.provider)}
                      disabled={saveState.kind !== "idle"}
                    >
                      {saveLabel}
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        setEditingProvider(null);
                        setDraftKey("");
                        setShowKey(false);
                      }}
                      disabled={saveState.kind !== "idle"}
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
                        setShowKey(false);
                      }}
                    >
                      {s.configured ? "Ändern" : "Setzen"}
                    </Button>
                    {s.configured ? (
                      <>
                        <Button
                          size="sm"
                          variant="secondary"
                          onClick={() => onTest(s.provider)}
                          disabled={test.kind === "running"}
                        >
                          {test.kind === "running"
                            ? "Teste…"
                            : "Verbindung testen"}
                        </Button>
                        {test.kind === "ok" && tested ? (
                          <span className="text-xs text-fg-faint">
                            {formatRelative(tested, now)}
                          </span>
                        ) : null}
                        <Button
                          size="sm"
                          variant="danger"
                          onClick={() => void onDelete(s.provider)}
                        >
                          Löschen
                        </Button>
                      </>
                    ) : null}
                  </>
                )}
              </div>
              {test.kind === "ok" ? (
                <Banner tone="success" dense>
                  ✓ Verbindung erfolgreich
                </Banner>
              ) : null}
              {test.kind === "error" ? (
                <Banner tone="error" dense>
                  {test.message}
                </Banner>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
