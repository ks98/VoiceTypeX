// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import type { Mode } from "../lib/types";
import { ipcCreateMode, ipcUpdateMode } from "../lib/tauri";
import { isWindows } from "../lib/platform";
import Button from "./Button";
import Banner from "./Banner";
import WhisperModelCards from "./WhisperModelCards";
import { computeBlockingReasons } from "./modeValidation";
import { LLM_SLOTS } from "../lib/llmSlots";
import { useT, type TranslateFn } from "../i18n";

// Local input classes — the ModeEditor has ~17 sites with different
// extra classes (font-mono, min-h-[120px], ...) that could only be
// replaced via repetitive markup using the generic `<Input />`
// component. Deliberately kept inline here, with a focus-visible
// ring for a11y parity with the Input component.
const inputCls =
  "bg-surface border border-outline rounded-md px-2 py-1.5 text-sm w-full text-fg placeholder:text-fg-faint focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 focus-visible:ring-offset-1 focus-visible:ring-offset-canvas focus:border-brand transition-colors";

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
    local_engine: null,
    ollama_model_tag: null,
    embedded_llm_slot: null,
    whisper_model_slot: null,
    initial_prompt: null,
    whisper_beam_size: null,
    injection_method: "clipboard",
    input: "voice",
    output: "insert",
    output_fallback: "replace",
    language: "de",
    system_prompt: null,
    temperature: null,
    top_p: null,
    repeat_penalty: null,
    max_tokens: null,
  };
}

export default function ModeEditor({
  initial,
  onClose,
  onSaved,
}: ModeEditorProps): JSX.Element {
  const t = useT();
  const isEdit = initial !== null;
  const [draft, setDraft] = useState<Mode>(initial ?? emptyMode());
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
  const isSelectionInput = draft.input === "selection";
  // Embedded llama-cpp-2 is not compiled on Windows (issue #1), so a
  // local-LLM mode there can only use Ollama. Force ollama on Windows; it
  // is persisted via `local_engine` on save, so a Windows-authored local
  // mode never carries `engine = "embedded"` (which would error at run time).
  const localEngine: "embedded" | "ollama" = isWindows()
    ? "ollama"
    : draft.local_engine === "ollama"
      ? "ollama"
      : "embedded";

  const blockingReasons = computeBlockingReasons(draft, isWindows()).map((r) =>
    t(`mode_editor.reason.${r}`),
  );
  const canSave = blockingReasons.length === 0;

  const baseline = JSON.stringify(initial ?? emptyMode());
  const isDirty = JSON.stringify(draft) !== baseline;

  const requestClose = () => {
    if (isDirty && !window.confirm(t("mode_editor.confirm_discard"))) {
      return;
    }
    onClose();
  };

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
        local_llm_model: null,
        whisper_model_slot: isLocalSTT ? draft.whisper_model_slot : null,
        initial_prompt: isLocalSTT ? draft.initial_prompt : null,
        whisper_beam_size: isLocalSTT ? draft.whisper_beam_size : null,
        system_prompt: needsSystemPrompt ? draft.system_prompt : null,
        // A voice mode always injects at the cursor; only selection
        // modes carry replace/append/prepend/auto.
        output: isSelectionInput ? draft.output : "insert",
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
            {isEdit
              ? t("mode_editor.title_edit", { name: draft.name })
              : t("mode_editor.title_new")}
          </h2>
        </div>

        <div className="p-5 flex flex-col gap-6 overflow-y-auto flex-1 min-h-0">
          {error ? <Banner tone="error">{error}</Banner> : null}

          <Section title={t("mode_editor.section.basics")}>
            <div className="grid grid-cols-2 gap-3">
              <Field
                label={t("mode_editor.id.label")}
                hint={t("mode_editor.id.hint")}
              >
                <input
                  className={`${inputCls} disabled:opacity-50`}
                  value={draft.id}
                  onChange={(e) => update("id", e.target.value)}
                  disabled={isEdit}
                />
                {!idValid && draft.id ? (
                  <div className="text-xs text-status-error">
                    {t("mode_editor.id.invalid")}
                  </div>
                ) : null}
              </Field>
              <Field label={t("mode_editor.name.label")}>
                <input
                  className={inputCls}
                  value={draft.name}
                  onChange={(e) => update("name", e.target.value)}
                />
              </Field>
            </div>
            <Field label={t("mode_editor.description.label")}>
              <input
                className={inputCls}
                value={draft.description}
                onChange={(e) => update("description", e.target.value)}
              />
            </Field>
            <Field
              label={t("mode_editor.input.label")}
              hint={t("mode_editor.input.hint")}
            >
              <select
                className={inputCls}
                value={draft.input}
                onChange={(e) => {
                  const v = e.target.value as "voice" | "selection";
                  // A selection mode must not stay on "insert" — that is
                  // the voice default and is hidden from the action
                  // picker. Bump it to "replace" on switch.
                  setDraft((d) => ({
                    ...d,
                    input: v,
                    output:
                      v === "selection" && d.output === "insert"
                        ? "replace"
                        : d.output,
                  }));
                }}
              >
                <option value="voice">
                  {t("mode_editor.input.opt_voice")}
                </option>
                <option value="selection">
                  {t("mode_editor.input.opt_selection")}
                </option>
              </select>
            </Field>
          </Section>

          <Section title={t("mode_editor.section.stt")}>
            <div className="grid grid-cols-2 gap-3">
              <Field label={t("mode_editor.stt.target")}>
                <select
                  className={inputCls}
                  value={draft.transcription}
                  onChange={(e) =>
                    update("transcription", e.target.value as "local" | "cloud")
                  }
                >
                  <option value="local">
                    {t("mode_editor.stt.opt_local")}
                  </option>
                  <option value="cloud">
                    {t("mode_editor.stt.opt_cloud")}
                  </option>
                </select>
              </Field>
              <Field
                label={t("mode_editor.stt.language.label")}
                hint={t("mode_editor.stt.language.hint")}
              >
                <input
                  className={inputCls}
                  value={draft.language ?? ""}
                  onChange={(e) => update("language", e.target.value || null)}
                  placeholder="de"
                />
              </Field>
            </div>

            {isCloudSTT ? (
              <Field label={t("mode_editor.stt.cloud_provider")}>
                <select
                  className={inputCls}
                  value={draft.cloud_stt_provider ?? ""}
                  onChange={(e) =>
                    update("cloud_stt_provider", e.target.value || null)
                  }
                >
                  <option value="">{t("mode_editor.stt.choose")}</option>
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
                  label={t("mode_editor.stt.whisper_slot.label")}
                  hint={t("mode_editor.stt.whisper_slot.hint")}
                >
                  <WhisperModelCards
                    value={draft.whisper_model_slot ?? null}
                    onChange={(slot) => update("whisper_model_slot", slot)}
                    hardware={null}
                    recommendedSlot={null}
                    t={t}
                    allowGlobal
                  />
                </Field>
                <Field
                  label={t("mode_editor.stt.initial_prompt.label")}
                  hint={t("mode_editor.stt.initial_prompt.hint")}
                >
                  <textarea
                    className={`${inputCls} min-h-[60px]`}
                    placeholder={t(
                      "mode_editor.stt.initial_prompt.placeholder",
                    )}
                    value={draft.initial_prompt ?? ""}
                    onChange={(e) =>
                      update("initial_prompt", e.target.value || null)
                    }
                  />
                </Field>
                <NumField
                  t={t}
                  label={t("mode_editor.stt.beam_size.label")}
                  hint={t("mode_editor.stt.beam_size.hint")}
                  value={draft.whisper_beam_size}
                  onChange={(v) => update("whisper_beam_size", v)}
                  min={1}
                  max={10}
                  step={1}
                  integer
                />
              </>
            ) : null}
          </Section>

          <Section title={t("mode_editor.section.llm")}>
            <Field label={t("mode_editor.llm.processing")}>
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
                <option value="none">{t("mode_editor.llm.opt_none")}</option>
                <option value="local">{t("mode_editor.llm.opt_local")}</option>
                <option value="cloud">{t("mode_editor.llm.opt_cloud")}</option>
              </select>
            </Field>

            {isLocalLLM ? (
              <>
                {/* Embedded engine is Linux/macOS-only (issue #1). On
                    Windows the engine is forced to Ollama, so we hide the
                    selector and show a short hint instead. */}
                {isWindows() ? (
                  <Field label={t("mode_editor.llm.engine.label")}>
                    <p className="text-xs text-fg-muted">
                      {t("mode_editor.llm.engine.windows_no_embedded")}
                    </p>
                  </Field>
                ) : (
                  <Field
                    label={t("mode_editor.llm.engine.label")}
                    hint={t("mode_editor.llm.engine.hint")}
                  >
                    <select
                      className={inputCls}
                      value={localEngine}
                      onChange={(e) => update("local_engine", e.target.value)}
                    >
                      <option value="embedded">
                        {t("mode_editor.llm.engine.opt_embedded")}
                      </option>
                      <option value="ollama">
                        {t("mode_editor.llm.engine.opt_ollama")}
                      </option>
                    </select>
                  </Field>
                )}

                {localEngine === "embedded" ? (
                  <Field
                    label={t("mode_editor.llm.embedded_slot.label")}
                    hint={t("mode_editor.llm.embedded_slot.hint")}
                  >
                    <select
                      className={inputCls}
                      value={draft.embedded_llm_slot ?? ""}
                      onChange={(e) =>
                        update("embedded_llm_slot", e.target.value || null)
                      }
                    >
                      <option value="">
                        {t("mode_editor.llm.embedded_slot.global")}
                      </option>
                      {LLM_SLOTS.map((s) => (
                        <option key={s.slot} value={s.slot}>
                          {t(`mode_editor.llm_slot.${s.keySuffix}`)}
                        </option>
                      ))}
                    </select>
                  </Field>
                ) : (
                  <Field
                    label={t("mode_editor.llm.ollama_tag.label")}
                    hint={t("mode_editor.llm.ollama_tag.hint")}
                  >
                    <input
                      className={`${inputCls} font-mono`}
                      placeholder="llama3.2:3b"
                      value={draft.ollama_model_tag ?? ""}
                      onChange={(e) =>
                        update("ollama_model_tag", e.target.value || null)
                      }
                    />
                  </Field>
                )}
              </>
            ) : null}

            {isCloudLLM ? (
              <div className="grid grid-cols-2 gap-3">
                <Field label={t("mode_editor.llm.cloud_provider")}>
                  <select
                    className={inputCls}
                    value={draft.cloud_llm_provider ?? ""}
                    onChange={(e) =>
                      update("cloud_llm_provider", e.target.value || null)
                    }
                  >
                    <option value="">{t("mode_editor.stt.choose")}</option>
                    {LLM_PROVIDERS.map((p) => (
                      <option key={p} value={p}>
                        {p}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field
                  label={t("mode_editor.llm.cloud_model.label")}
                  hint={t("mode_editor.llm.cloud_model.hint")}
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
                label={t("mode_editor.llm.system_prompt.label")}
                hint={t("mode_editor.llm.system_prompt.hint")}
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

          {needsSystemPrompt ? (
            <Section
              title={t("mode_editor.section.sampling")}
              collapsible
              open={samplingOpen}
              onToggle={() => setSamplingOpen((o) => !o)}
            >
              <p className="text-xs text-fg-faint">
                {t("mode_editor.sampling.intro")}
              </p>
              <div className="grid grid-cols-2 gap-3">
                <NumField
                  t={t}
                  label="temperature"
                  hint={t("mode_editor.sampling.temperature.hint")}
                  value={draft.temperature}
                  onChange={(v) => update("temperature", v)}
                  min={0}
                  max={2}
                  step={0.1}
                />
                <NumField
                  t={t}
                  label="top_p"
                  hint={t("mode_editor.sampling.top_p.hint")}
                  value={draft.top_p}
                  onChange={(v) => update("top_p", v)}
                  min={0}
                  max={1}
                  step={0.05}
                />
                <NumField
                  t={t}
                  label="repeat_penalty"
                  hint={t("mode_editor.sampling.repeat_penalty.hint")}
                  value={draft.repeat_penalty}
                  onChange={(v) => update("repeat_penalty", v)}
                  min={0.5}
                  max={2}
                  step={0.05}
                />
                <NumField
                  t={t}
                  label="max_tokens"
                  hint={t("mode_editor.sampling.max_tokens.hint")}
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

          <Section title={t("mode_editor.section.output")}>
            <Field
              label={t("mode_editor.output.inject.label")}
              hint={t("mode_editor.output.inject.hint")}
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
                <option value="clipboard">
                  {t("mode_editor.output.inject.opt_clipboard")}
                </option>
                <option value="keystrokes">
                  {t("mode_editor.output.inject.opt_keystrokes")}
                </option>
              </select>
            </Field>

            {isSelectionInput ? (
              <>
                <Field
                  label={t("mode_editor.output.action.label")}
                  hint={t("mode_editor.output.action.hint")}
                >
                  <select
                    className={inputCls}
                    value={draft.output}
                    onChange={(e) =>
                      update("output", e.target.value as Mode["output"])
                    }
                  >
                    <option value="replace">
                      {t("mode_editor.output.action.opt_replace")}
                    </option>
                    <option value="append">
                      {t("mode_editor.output.action.opt_append")}
                    </option>
                    <option value="prepend">
                      {t("mode_editor.output.action.opt_prepend")}
                    </option>
                    <option value="auto">
                      {t("mode_editor.output.action.opt_auto")}
                    </option>
                  </select>
                </Field>
                {draft.output === "auto" ? (
                  <Field
                    label={t("mode_editor.output.fallback.label")}
                    hint={t("mode_editor.output.fallback.hint")}
                  >
                    <select
                      className={inputCls}
                      value={draft.output_fallback}
                      onChange={(e) =>
                        update(
                          "output_fallback",
                          e.target.value as Mode["output_fallback"],
                        )
                      }
                    >
                      <option value="replace">
                        {t("mode_editor.output.action.opt_replace")}
                      </option>
                      <option value="append">
                        {t("mode_editor.output.action.opt_append")}
                      </option>
                      <option value="prepend">
                        {t("mode_editor.output.action.opt_prepend")}
                      </option>
                    </select>
                  </Field>
                ) : null}
              </>
            ) : null}
          </Section>
        </div>

        <div className="p-5 border-t border-outline flex justify-between items-center gap-3 shrink-0 bg-surface">
          <div className="text-xs text-fg-faint min-w-0 flex-1">
            {blockingReasons.length > 0 ? (
              <span>
                {t("mode_editor.footer.blocking_prefix")}{" "}
                <span className="text-fg-muted">
                  {blockingReasons.slice(0, 3).join(", ")}
                  {blockingReasons.length > 3
                    ? ` ${t("mode_editor.footer.blocking_more", {
                        count: blockingReasons.length - 3,
                      })}`
                    : ""}
                </span>
              </span>
            ) : null}
          </div>
          <Button variant="secondary" onClick={requestClose}>
            {t("mode_editor.btn.cancel")}
          </Button>
          <Button onClick={() => void onSave()} disabled={!canSave || saving}>
            {saving
              ? t("mode_editor.btn.saving")
              : isEdit
                ? t("mode_editor.btn.save_edit")
                : t("mode_editor.btn.save_new")}
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
  t: TranslateFn;
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
  t,
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
        placeholder={t("mode_editor.numfield.placeholder")}
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
          {t("mode_editor.numfield.range", { min, max })}
        </div>
      ) : null}
    </Field>
  );
}
