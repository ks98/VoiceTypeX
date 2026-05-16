// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import type { Mode } from "../lib/types";
import { ipcCreateMode, ipcUpdateMode } from "../lib/tauri";

interface ModeEditorProps {
  initial: Mode | null;
  onClose: () => void;
  onSaved: () => void;
}

const STT_PROVIDERS = ["xai", "openai", "groq", "deepgram"];
const LLM_PROVIDERS = ["xai", "openai", "anthropic"];

function emptyMode(): Mode {
  return {
    id: "",
    name: "",
    description: "",
    transcription: "local",
    processing: "none",
    cloud_stt_provider: null,
    cloud_llm_provider: null,
    cloud_llm_model: null,
    local_llm_model: null,
    injection_method: "clipboard",
    language: "de",
    system_prompt: null,
  };
}

export default function ModeEditor({
  initial,
  onClose,
  onSaved,
}: ModeEditorProps): JSX.Element {
  const isEdit = initial !== null;
  const [draft, setDraft] = useState<Mode>(initial ?? emptyMode());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const update = <K extends keyof Mode>(key: K, value: Mode[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const idValid = /^[a-zA-Z0-9_-]+$/.test(draft.id);
  const needsSttProvider = draft.transcription === "cloud";
  const needsLlmProvider = draft.processing === "cloud";
  const needsSystemPrompt = draft.processing !== "none";
  const needsLocalModel = draft.processing === "local";

  const canSave =
    draft.id.length > 0 &&
    idValid &&
    draft.name.length > 0 &&
    (!needsSttProvider || !!draft.cloud_stt_provider) &&
    (!needsLlmProvider || !!draft.cloud_llm_provider) &&
    (!needsLocalModel || !!draft.local_llm_model) &&
    (!needsSystemPrompt ||
      (draft.system_prompt !== null && draft.system_prompt.length > 0));

  const onSave = async () => {
    setSaving(true);
    setError(null);
    try {
      // Bereinige conditional Felder, sodass keine Leichen im TOML landen.
      const cleaned: Mode = {
        ...draft,
        cloud_stt_provider: needsSttProvider ? draft.cloud_stt_provider : null,
        cloud_llm_provider: needsLlmProvider ? draft.cloud_llm_provider : null,
        cloud_llm_model: needsLlmProvider ? draft.cloud_llm_model : null,
        local_llm_model: needsLocalModel ? draft.local_llm_model : null,
        system_prompt: needsSystemPrompt ? draft.system_prompt : null,
      };
      if (isEdit) {
        await ipcUpdateMode(cleaned);
      } else {
        await ipcCreateMode(cleaned);
      }
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4">
      <div className="bg-slate-950 border border-slate-700 rounded-lg max-w-3xl w-full max-h-[90vh] overflow-auto">
        <div className="p-5 border-b border-slate-800">
          <h2 className="text-lg font-semibold text-slate-100">
            {isEdit ? `Modus bearbeiten: ${draft.name}` : "Neuer Modus"}
          </h2>
        </div>

        <div className="p-5 flex flex-col gap-4">
          {error ? (
            <div className="rounded-md bg-red-900/30 border border-red-700 px-3 py-2 text-sm text-red-300">
              {error}
            </div>
          ) : null}

          <div className="grid grid-cols-2 gap-3">
            <Field label="ID" hint="kurz, ohne Leerzeichen, [a-zA-Z0-9_-]">
              <input
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full disabled:opacity-50"
                value={draft.id}
                onChange={(e) => update("id", e.target.value)}
                disabled={isEdit}
              />
              {!idValid && draft.id ? (
                <div className="text-xs text-red-400">
                  Nur a-z, 0-9, _ und - erlaubt
                </div>
              ) : null}
            </Field>
            <Field label="Anzeigename">
              <input
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
                value={draft.name}
                onChange={(e) => update("name", e.target.value)}
              />
            </Field>
          </div>

          <Field label="Beschreibung">
            <input
              className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
              value={draft.description}
              onChange={(e) => update("description", e.target.value)}
            />
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Transkription">
              <select
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
                value={draft.transcription}
                onChange={(e) =>
                  update("transcription", e.target.value as "local" | "cloud")
                }
              >
                <option value="local">local (whisper.cpp)</option>
                <option value="cloud">cloud (Provider)</option>
              </select>
            </Field>
            <Field label="Nachbearbeitung">
              <select
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
                value={draft.processing}
                onChange={(e) =>
                  update(
                    "processing",
                    e.target.value as "none" | "local" | "cloud",
                  )
                }
              >
                <option value="none">none (passthrough)</option>
                <option value="local">local (Ollama)</option>
                <option value="cloud">cloud (LLM)</option>
              </select>
            </Field>
          </div>

          {needsSttProvider ? (
            <Field label="Cloud-STT-Provider">
              <select
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
                value={draft.cloud_stt_provider ?? ""}
                onChange={(e) =>
                  update("cloud_stt_provider", e.target.value || null)
                }
              >
                <option value="">— wählen —</option>
                {STT_PROVIDERS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </Field>
          ) : null}

          {needsLlmProvider ? (
            <div className="grid grid-cols-2 gap-3">
              <Field label="Cloud-LLM-Provider">
                <select
                  className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
                  value={draft.cloud_llm_provider ?? ""}
                  onChange={(e) =>
                    update("cloud_llm_provider", e.target.value || null)
                  }
                >
                  <option value="">— wählen —</option>
                  {LLM_PROVIDERS.map((p) => (
                    <option key={p} value={p}>
                      {p}
                    </option>
                  ))}
                </select>
              </Field>
              <Field
                label="Modell-ID"
                hint="z.B. grok-4-fast-non-reasoning, gpt-4o-mini, claude-sonnet-4-6"
              >
                <input
                  className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full font-mono"
                  value={draft.cloud_llm_model ?? ""}
                  onChange={(e) =>
                    update("cloud_llm_model", e.target.value || null)
                  }
                />
              </Field>
            </div>
          ) : null}

          {needsLocalModel ? (
            <Field
              label="Lokales LLM-Modell (Ollama-Tag)"
              hint="z.B. qwen2.5:7b, llama3.1:8b"
            >
              <input
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full font-mono"
                value={draft.local_llm_model ?? ""}
                onChange={(e) =>
                  update("local_llm_model", e.target.value || null)
                }
              />
            </Field>
          ) : null}

          <div className="grid grid-cols-2 gap-3">
            <Field label="Inject-Methode">
              <select
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
                value={draft.injection_method}
                onChange={(e) =>
                  update(
                    "injection_method",
                    e.target.value as "clipboard" | "keystrokes",
                  )
                }
              >
                <option value="clipboard">clipboard (empfohlen)</option>
                <option value="keystrokes">
                  keystrokes (in Phase 1 ignored)
                </option>
              </select>
            </Field>
            <Field label="Sprache (ISO-Code)">
              <input
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full"
                value={draft.language ?? ""}
                onChange={(e) => update("language", e.target.value || null)}
                placeholder="de"
              />
            </Field>
          </div>

          {needsSystemPrompt ? (
            <Field
              label="System-Prompt"
              hint="Wird als System-Message ans LLM geschickt."
            >
              <textarea
                className="bg-slate-900 border border-slate-700 rounded px-2 py-1.5 text-sm w-full min-h-[120px] font-mono"
                value={draft.system_prompt ?? ""}
                onChange={(e) =>
                  update("system_prompt", e.target.value || null)
                }
              />
            </Field>
          ) : null}
        </div>

        <div className="p-5 border-t border-slate-800 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 rounded bg-slate-800 hover:bg-slate-700 text-sm"
          >
            Abbrechen
          </button>
          <button
            type="button"
            onClick={() => void onSave()}
            disabled={!canSave || saving}
            className="px-4 py-2 rounded bg-brand-700 hover:bg-brand-500 disabled:bg-slate-800 disabled:text-slate-500 text-sm"
          >
            {saving ? "Speichere…" : isEdit ? "Speichern" : "Anlegen"}
          </button>
        </div>
      </div>
    </div>
  );
}

interface FieldProps {
  label: string;
  hint?: string;
  children: React.ReactNode;
}

function Field({ label, hint, children }: FieldProps): JSX.Element {
  return (
    <div className="flex flex-col gap-1">
      <label className="text-xs font-medium text-slate-400">{label}</label>
      {children}
      {hint ? <div className="text-xs text-slate-600">{hint}</div> : null}
    </div>
  );
}
