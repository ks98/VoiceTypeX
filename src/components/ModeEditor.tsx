// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import type { Mode } from "../lib/types";
import { ipcCreateMode, ipcUpdateMode } from "../lib/tauri";
import Button from "./Button";
import Banner from "./Banner";

// Lokale Input-Klassen — der ModeEditor hat ~17 Sites mit unterschiedlichen
// Zusatz-Klassen (font-mono, min-h-[120px], ...), die mit der generischen
// <Input />-Komponente nur via repetitivem Markup ersetzbar waeren. Hier
// bewusst inline gelassen, mit Focus-Visible-Ring fuer A11y-Paritaet zur
// Input-Komponente. Konsolidierung der Sites ist eigenes Refactoring.
const inputCls =
  "bg-surface border border-outline rounded-md px-2 py-1.5 text-sm w-full text-fg placeholder:text-fg-faint focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 focus-visible:ring-offset-1 focus-visible:ring-offset-canvas focus:border-brand transition-colors";

interface ModeEditorProps {
  initial: Mode | null;
  onClose: () => void;
  onSaved: () => void;
}

const STT_PROVIDERS = ["xai", "openai", "groq", "deepgram"];
const LLM_PROVIDERS = ["xai", "openai", "anthropic"];

// Slot-Listen spiegeln die Backend-Mappings (ModelSlot::from_setting +
// LlmModelSlot::from_setting). Gleiche Reihenfolge wie in Settings.tsx,
// damit der User dieselbe Wahl trifft, egal von wo er kommt.
const WHISPER_SLOTS: Array<{ value: string; label: string }> = [
  {
    value: "large-v3-turbo-q8_0",
    label: "large-v3-turbo-q8_0 (~874 MB, Default)",
  },
  {
    value: "large-v3-turbo-german-q5_0",
    label: "large-v3-turbo-german-q5_0 (~574 MB, DE Pro)",
  },
  {
    value: "large-v3-turbo-q5_0",
    label: "large-v3-turbo-q5_0 (~547 MB, Light)",
  },
  { value: "small-q5_1", label: "small-q5_1 (~181 MB, 4 GB RAM)" },
  { value: "large-v3-turbo", label: "large-v3-turbo F16 (~1,6 GB)" },
];

const LLM_SLOTS: Array<{ value: string; label: string }> = [
  {
    value: "gemma4-e4b-it-q5_k_m",
    label: "Gemma 4 E4B-IT Q5_K_M (~5,1 GB, Pro)",
  },
  {
    value: "gemma4-e2b-it-q5_k_m",
    label: "Gemma 4 E2B-IT Q5_K_M (~3,1 GB, Mittel)",
  },
  {
    value: "gemma3-1b-it-q5_k_m",
    label: "Gemma 3 1B-IT Q5_K_M (~851 MB, Light)",
  },
  {
    value: "gemma3-4b-it-q5_k_m",
    label: "Gemma 3 4B-IT Q5_K_M (~2,8 GB, Legacy)",
  },
  {
    value: "llama3.2-1b-instruct-q5_k_m",
    label: "Llama 3.2 1B-Instruct Q5_K_M (~912 MB, EN)",
  },
  {
    value: "qwen2.5-1.5b-instruct-q5_k_m",
    label: "Qwen 2.5 1.5B-Instruct Q5_K_M (~1,3 GB, Code)",
  },
];

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
    ollama_model_tag: null,
    embedded_llm_slot: null,
    whisper_model_slot: null,
    initial_prompt: null,
    injection_method: "clipboard",
    language: "de",
    system_prompt: null,
    temperature: null,
    top_p: null,
    repeat_penalty: null,
    max_tokens: null,
  };
}

// Spiegelt die Range-Checks aus `Mode::validate()` in `src-tauri/src/core/modes.rs`.
// Wenn das Backend strenger wird, hier mitziehen — sonst sieht der User
// erst beim Speichern den Fehler.
function inRange(
  value: number | null,
  min: number,
  max: number,
): boolean {
  if (value === null) return true;
  if (!Number.isFinite(value)) return false;
  return value >= min && value <= max;
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
  // Sampling-Section ist default-collapsed — **außer** der initiale Modus
  // hat schon Werte gesetzt. Sonst sieht der User beim Edit nicht, dass
  // sein temperature=0.3 noch da ist, und denkt, er müsse es neu setzen.
  const [samplingOpen, setSamplingOpen] = useState<boolean>(
    initial !== null &&
      (initial.temperature !== null ||
        initial.top_p !== null ||
        initial.repeat_penalty !== null ||
        initial.max_tokens !== null),
  );

  const update = <K extends keyof Mode>(key: K, value: Mode[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const idValid = /^[a-zA-Z0-9_-]+$/.test(draft.id);
  const isLocalSTT = draft.transcription === "local";
  const isCloudSTT = draft.transcription === "cloud";
  const isLocalLLM = draft.processing === "local";
  const isCloudLLM = draft.processing === "cloud";
  const needsSystemPrompt = draft.processing !== "none";
  // Engine-Default ist Ollama (Backward-Compat), wenn das Feld leer ist.
  const localEngine: "embedded" | "ollama" =
    draft.local_engine === "embedded" ? "embedded" : "ollama";
  const needsOllamaTag = isLocalLLM && localEngine === "ollama";

  const samplingValid =
    inRange(draft.temperature, 0, 2) &&
    inRange(draft.top_p, 0, 1) &&
    inRange(draft.repeat_penalty, 0.5, 2) &&
    inRange(draft.max_tokens, 1, 8192);

  // Statt black-box-Button: explizit benennen, was noch fehlt. Der erste
  // Eintrag wird im Footer angezeigt.
  const blockingReasons: string[] = [];
  if (draft.id.length === 0) blockingReasons.push("ID");
  else if (!idValid) blockingReasons.push("ID (nur a-z, 0-9, _, -)");
  if (draft.name.length === 0) blockingReasons.push("Anzeigename");
  if (isCloudSTT && !draft.cloud_stt_provider)
    blockingReasons.push("Cloud-STT-Provider");
  if (isCloudLLM && !draft.cloud_llm_provider)
    blockingReasons.push("Cloud-LLM-Provider");
  if (needsOllamaTag && !draft.ollama_model_tag)
    blockingReasons.push("Ollama-Modell-Tag");
  if (
    needsSystemPrompt &&
    (draft.system_prompt === null || draft.system_prompt.length === 0)
  )
    blockingReasons.push("System-Prompt");
  if (!samplingValid)
    blockingReasons.push("Sampling-Wert(e) außerhalb erlaubter Bereiche");
  const canSave = blockingReasons.length === 0;

  // Dirty-Check fuer Escape/Backdrop — Vergleich gegen Initial (oder leere
  // Vorlage). JSON-stringify ist genug fuer eine flache Struktur und
  // billiger als Per-Feld-Vergleiche.
  const baseline = JSON.stringify(initial ?? emptyMode());
  const isDirty = JSON.stringify(draft) !== baseline;

  const requestClose = () => {
    if (
      isDirty &&
      !window.confirm("Ungespeicherte Änderungen verwerfen?")
    ) {
      return;
    }
    onClose();
  };

  // Escape-Handler — Browser-Default fuer Modals fehlt, ohne explizite
  // Bindung schliesst nur der Backdrop-Klick (den wir bewusst nicht
  // handhaben). Mit Dirty-Check.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        requestClose();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isDirty]);

  const onSave = async () => {
    setSaving(true);
    setError(null);
    try {
      // Felder, die fuer den aktuellen Modus-Typ irrelevant sind, beim
      // Speichern auf null setzen — sonst landen veraltete Werte aus
      // einem frueheren Modus-Typ im TOML.
      const cleaned: Mode = {
        ...draft,
        cloud_stt_provider: isCloudSTT ? draft.cloud_stt_provider : null,
        cloud_llm_provider: isCloudLLM ? draft.cloud_llm_provider : null,
        cloud_llm_model: isCloudLLM ? draft.cloud_llm_model : null,
        local_engine: isLocalLLM ? localEngine : null,
        ollama_model_tag:
          isLocalLLM && localEngine === "ollama"
            ? draft.ollama_model_tag
            : null,
        embedded_llm_slot:
          isLocalLLM && localEngine === "embedded"
            ? draft.embedded_llm_slot
            : null,
        // Deprecated-Feld leeren — Backend migriert ohnehin von hier
        // nach `ollama_model_tag`, aber sauberer ist explizit null.
        local_llm_model: null,
        whisper_model_slot: isLocalSTT ? draft.whisper_model_slot : null,
        initial_prompt: isLocalSTT ? draft.initial_prompt : null,
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
    <div
      className="fixed inset-0 bg-black/60 backdrop-blur-sm flex items-center justify-center z-50 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="mode-editor-title"
    >
      <div className="bg-surface border border-outline rounded-lg max-w-3xl w-full max-h-[90vh] shadow-2xl flex flex-col overflow-hidden">
        <div className="p-5 border-b border-outline shrink-0">
          <h2 id="mode-editor-title" className="text-lg font-semibold text-fg">
            {isEdit ? `Modus bearbeiten: ${draft.name}` : "Neuer Modus"}
          </h2>
        </div>

        {/* Body: scrollt unabhaengig vom Sticky-Footer. */}
        <div className="p-5 flex flex-col gap-6 overflow-y-auto flex-1 min-h-0">
          {error ? <Banner tone="error">{error}</Banner> : null}

          {/* ──────────────────────── Section 1 — Basis ────────────────────── */}
          <Section title="Basis">
            <div className="grid grid-cols-2 gap-3">
              <Field
                label="ID"
                hint="kurz, ohne Leerzeichen, [a-zA-Z0-9_-]"
              >
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
          </Section>

          {/* ─────────────────── Section 2 — Transkription ────────────────── */}
          <Section title="Transkription (Speech-to-Text)">
            <div className="grid grid-cols-2 gap-3">
              <Field label="STT-Ziel">
                <select
                  className={inputCls}
                  value={draft.transcription}
                  onChange={(e) =>
                    update(
                      "transcription",
                      e.target.value as "local" | "cloud",
                    )
                  }
                >
                  <option value="local">local (whisper.cpp)</option>
                  <option value="cloud">cloud (Provider)</option>
                </select>
              </Field>
              <Field
                label="Sprache (ISO-Code)"
                hint="z.B. de, en. Leer = Whisper detektiert automatisch."
              >
                <input
                  className={inputCls}
                  value={draft.language ?? ""}
                  onChange={(e) =>
                    update("language", e.target.value || null)
                  }
                  placeholder="de"
                />
              </Field>
            </div>

            {isCloudSTT ? (
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

            {isLocalSTT ? (
              <>
                <Field
                  label="Whisper-Modell"
                  hint="Override pro Modus. „Global verwenden“ nutzt den in den Einstellungen gewählten Default-Slot."
                >
                  <select
                    className={inputCls}
                    value={draft.whisper_model_slot ?? ""}
                    onChange={(e) =>
                      update(
                        "whisper_model_slot",
                        e.target.value || null,
                      )
                    }
                  >
                    <option value="">Global verwenden</option>
                    {WHISPER_SLOTS.map((s) => (
                      <option key={s.value} value={s.value}>
                        {s.label}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field
                  label="Initial-Prompt (Glossar)"
                  hint="Optional. Hinweise auf Eigennamen, Fachbegriffe oder Schreibweisen, die Whisper als Kontext bekommen soll."
                >
                  <textarea
                    className={`${inputCls} min-h-[60px]`}
                    placeholder="z.B. Eigennamen: Wittenstein, OPC-UA, Tauri."
                    value={draft.initial_prompt ?? ""}
                    onChange={(e) =>
                      update("initial_prompt", e.target.value || null)
                    }
                  />
                </Field>
              </>
            ) : null}
          </Section>

          {/* ────────────────── Section 3 — Nachbearbeitung ───────────────── */}
          <Section title="Nachbearbeitung (LLM)">
            <Field label="Postprocessing">
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
                <option value="local">local (embedded oder Ollama)</option>
                <option value="cloud">cloud (LLM-Provider)</option>
              </select>
            </Field>

            {isLocalLLM ? (
              <>
                <Field
                  label="Lokale Engine"
                  hint="embedded = llama-cpp-2 im VoiceTypeX-Prozess (kein Daemon). ollama = externer Ollama-Daemon."
                >
                  <select
                    className={inputCls}
                    value={localEngine}
                    onChange={(e) =>
                      update("local_engine", e.target.value)
                    }
                  >
                    <option value="embedded">embedded (llama-cpp-2)</option>
                    <option value="ollama">ollama (extern)</option>
                  </select>
                </Field>

                {localEngine === "embedded" ? (
                  <Field
                    label="Embedded-GGUF-Modell"
                    hint="Override pro Modus. „Global verwenden“ nutzt den in den Einstellungen gewählten Default-Slot."
                  >
                    <select
                      className={inputCls}
                      value={draft.embedded_llm_slot ?? ""}
                      onChange={(e) =>
                        update(
                          "embedded_llm_slot",
                          e.target.value || null,
                        )
                      }
                    >
                      <option value="">Global verwenden</option>
                      {LLM_SLOTS.map((s) => (
                        <option key={s.value} value={s.value}>
                          {s.label}
                        </option>
                      ))}
                    </select>
                  </Field>
                ) : (
                  <Field
                    label="Ollama-Modell-Tag"
                    hint="Pflichtfeld bei engine=ollama. Beispiele: llama3.2:3b, qwen2.5:7b, gemma3:4b."
                  >
                    <input
                      className={`${inputCls} font-mono`}
                      placeholder="llama3.2:3b"
                      value={draft.ollama_model_tag ?? ""}
                      onChange={(e) =>
                        update(
                          "ollama_model_tag",
                          e.target.value || null,
                        )
                      }
                    />
                  </Field>
                )}
              </>
            ) : null}

            {isCloudLLM ? (
              <div className="grid grid-cols-2 gap-3">
                <Field label="Cloud-LLM-Provider">
                  <select
                    className={inputCls}
                    value={draft.cloud_llm_provider ?? ""}
                    onChange={(e) =>
                      update(
                        "cloud_llm_provider",
                        e.target.value || null,
                      )
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

            {needsSystemPrompt ? (
              <Field
                label="System-Prompt"
                hint="Wird als System-Message ans LLM geschickt. Beschreibt die Aufgabe (z.B. „Korrigiere die Grammatik, lass den Stil unverändert“)."
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
          </Section>

          {/* ─────────────────── Section 4 — Sampling ─────────────────────── */}
          {needsSystemPrompt ? (
            <Section
              title="Sampling-Parameter"
              collapsible
              open={samplingOpen}
              onToggle={() => setSamplingOpen((o) => !o)}
            >
              <p className="text-xs text-fg-faint">
                Leer = Provider-/Engine-Default. Nur ändern, wenn du weißt,
                was die Werte tun.
              </p>
              <div className="grid grid-cols-2 gap-3">
                <NumField
                  label="temperature"
                  hint="0.0–2.0. Niedriger = deterministischer."
                  value={draft.temperature}
                  onChange={(v) => update("temperature", v)}
                  min={0}
                  max={2}
                  step={0.1}
                />
                <NumField
                  label="top_p"
                  hint="0.0–1.0. Nucleus-Sampling-Schwelle."
                  value={draft.top_p}
                  onChange={(v) => update("top_p", v)}
                  min={0}
                  max={1}
                  step={0.05}
                />
                <NumField
                  label="repeat_penalty"
                  hint="0.5–2.0. >1.0 bestraft Wiederholungen."
                  value={draft.repeat_penalty}
                  onChange={(v) => update("repeat_penalty", v)}
                  min={0.5}
                  max={2}
                  step={0.05}
                />
                <NumField
                  label="max_tokens"
                  hint="1–8192. Output-Token-Limit."
                  value={draft.max_tokens}
                  onChange={(v) => update("max_tokens", v)}
                  min={1}
                  max={8192}
                  step={1}
                  integer
                />
              </div>
            </Section>
          ) : null}

          {/* ────────────────────── Section 5 — Output ────────────────────── */}
          <Section title="Output">
            <Field
              label="Inject-Methode"
              hint="clipboard fügt per Strg+V ein (schnell, benötigt funktionierende Zwischenablage). keystrokes simuliert Tastenanschläge (langsamer, aber kein Clipboard-Bedarf)."
            >
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
                <option value="keystrokes">keystrokes</option>
              </select>
            </Field>
          </Section>
        </div>

        {/* Sticky-Footer: bleibt sichtbar, auch wenn der Body scrollt. */}
        <div className="p-5 border-t border-outline flex justify-between items-center gap-3 shrink-0 bg-surface">
          <div className="text-xs text-fg-faint min-w-0 flex-1">
            {blockingReasons.length > 0 ? (
              <span>
                Noch nötig:{" "}
                <span className="text-fg-muted">
                  {blockingReasons.slice(0, 3).join(", ")}
                  {blockingReasons.length > 3
                    ? ` (+${blockingReasons.length - 3})`
                    : ""}
                </span>
              </span>
            ) : null}
          </div>
          <Button variant="secondary" onClick={requestClose}>
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

interface SectionProps {
  title: string;
  collapsible?: boolean;
  open?: boolean;
  onToggle?: () => void;
  children: React.ReactNode;
}

function Section({
  title,
  collapsible,
  open,
  onToggle,
  children,
}: SectionProps): JSX.Element {
  if (collapsible) {
    return (
      <section className="flex flex-col gap-3 rounded-md border border-outline/60 bg-surface/40 p-3">
        <button
          type="button"
          onClick={onToggle}
          aria-expanded={open}
          className="flex items-center justify-between text-left text-sm font-semibold text-fg hover:text-brand transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 rounded"
        >
          <span>{title}</span>
          <span className="text-xs text-fg-muted">{open ? "▾" : "▸"}</span>
        </button>
        {open ? <div className="flex flex-col gap-3">{children}</div> : null}
      </section>
    );
  }
  return (
    <section className="flex flex-col gap-3 rounded-md border border-outline/60 bg-surface/40 p-3">
      <h3 className="text-sm font-semibold text-fg border-b border-outline/40 pb-2">
        {title}
      </h3>
      <div className="flex flex-col gap-3">{children}</div>
    </section>
  );
}

interface FieldProps {
  label: string;
  hint?: string | undefined;
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

interface NumFieldProps {
  label: string;
  hint?: string;
  value: number | null;
  onChange: (v: number | null) => void;
  min: number;
  max: number;
  step: number;
  integer?: boolean;
}

function NumField({
  label,
  hint,
  value,
  onChange,
  min,
  max,
  step,
  integer,
}: NumFieldProps): JSX.Element {
  const outOfRange =
    value !== null && Number.isFinite(value) && (value < min || value > max);
  return (
    <Field label={label} hint={hint}>
      <input
        type="number"
        min={min}
        max={max}
        step={step}
        className={`${inputCls} ${
          outOfRange ? "border-status-error focus:border-status-error" : ""
        }`}
        placeholder="(Default)"
        value={value === null ? "" : String(value)}
        onChange={(e) => {
          const raw = e.target.value.trim();
          if (raw === "") {
            onChange(null);
            return;
          }
          const n = integer ? parseInt(raw, 10) : parseFloat(raw);
          if (Number.isNaN(n)) return;
          onChange(n);
        }}
      />
      {outOfRange ? (
        <div className="text-xs text-status-error">
          Erlaubt: {min} – {max}
        </div>
      ) : null}
    </Field>
  );
}
