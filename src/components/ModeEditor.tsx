// SPDX-License-Identifier: GPL-3.0-or-later
import { useState } from "react";
import type { Mode } from "../lib/types";
import { ipcCreateMode, ipcUpdateMode } from "../lib/tauri";
import Button from "./Button";

interface ModeEditorProps {
  initial: Mode | null;
  onClose: () => void;
  onSaved: () => void;
}

const STT_PROVIDERS = ["xai", "openai", "groq", "deepgram"];
const LLM_PROVIDERS = ["xai", "openai", "anthropic"];

const inputCls =
  "bg-surface border border-outline rounded-md px-2 py-1.5 text-sm w-full text-fg placeholder:text-fg-faint focus:outline-none focus:border-brand focus:ring-1 focus:ring-brand/40";

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
    local_engine: null,
    injection_method: "clipboard",
    language: "de",
    system_prompt: null,
    temperature: null,
    top_p: null,
    repeat_penalty: null,
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
    <div className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-surface border border-outline rounded-lg max-w-3xl w-full max-h-[90vh] overflow-auto shadow-2xl">
        <div className="p-5 border-b border-outline">
          <h2 className="text-lg font-semibold text-fg">
            {isEdit ? `Modus bearbeiten: ${draft.name}` : "Neuer Modus"}
          </h2>
        </div>

        <div className="p-5 flex flex-col gap-4">
          {error ? (
            <div className="rounded-md bg-status-error/10 border border-status-error/40 px-3 py-2 text-sm text-status-error">
              {error}
            </div>
          ) : null}

          <div className="grid grid-cols-2 gap-3">
            <Field label="ID" hint="kurz, ohne Leerzeichen, [a-zA-Z0-9_-]">
              <input
                className={`${inputCls} disabled:opacity-50`}
                value={draft.id}
                onChange={(e) => update("id", e.target.value)}
                disabled={isEdit}
              />
              {!idValid && draft.id ? (
                <div className="text-xs text-status-error">
                  Nur a-z, 0-9, _ und - erlaubt
                </div>
              ) : null}
            </Field>
            <Field label="Anzeigename">
              <input
                className={inputCls}
                value={draft.name}
                onChange={(e) => update("name", e.target.value)}
              />
            </Field>
          </div>

          <Field label="Beschreibung">
            <input
              className={inputCls}
              value={draft.description}
              onChange={(e) => update("description", e.target.value)}
            />
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label="Transkription">
              <select
                className={inputCls}
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
                className={inputCls}
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
                className={inputCls}
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
                  className={inputCls}
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
                  className={`${inputCls} font-mono`}
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
                className={`${inputCls} font-mono`}
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
                className={inputCls}
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
                className={inputCls}
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
                className={`${inputCls} min-h-[120px] font-mono`}
                value={draft.system_prompt ?? ""}
                onChange={(e) =>
                  update("system_prompt", e.target.value || null)
                }
              />
            </Field>
          ) : null}
        </div>

        <div className="p-5 border-t border-outline flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose}>
            Abbrechen
          </Button>
          <Button onClick={() => void onSave()} disabled={!canSave || saving}>
            {saving ? "Speichere…" : isEdit ? "Speichern" : "Anlegen"}
          </Button>
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
      <label className="text-xs font-medium text-fg-muted">{label}</label>
      {children}
      {hint ? <div className="text-xs text-fg-faint">{hint}</div> : null}
    </div>
  );
}
