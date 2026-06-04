// SPDX-License-Identifier: GPL-3.0-or-later
//
// Local Whisper model picker rendered as comparison cards: each slot
// shows a tempo + accuracy bar, its disk/RAM footprint, a DE badge for
// the German fine-tunes, and a "recommended for your hardware" marker.
// Replaces the bare <select> so a user can rank weaker vs stronger
// models at a glance (issue: model overview). Shared by Settings (global
// default) and ModeEditor (per-mode override, with a "use global" card).

import type { TranslateFn } from "../i18n";
import type { HardwareReport } from "../lib/tauri";
import { WHISPER_MODELS, type WhisperModelMeta } from "../lib/whisperModels";

interface WhisperModelCardsProps {
  /** Selected slot, or null = "use global" (override context only). */
  value: string | null;
  onChange: (slot: string | null) => void;
  hardware: HardwareReport | null;
  /** Slot the recommender suggests for this hardware/language, if any. */
  recommendedSlot: string | null;
  t: TranslateFn;
  /** Show a leading "use global default" card (ModeEditor override). */
  allowGlobal?: boolean;
}

export default function WhisperModelCards({
  value,
  onChange,
  hardware,
  recommendedSlot,
  t,
  allowGlobal = false,
}: WhisperModelCardsProps): JSX.Element {
  const detectedRam = hardware?.total_ram_gb ?? 0;
  return (
    <div className="flex flex-col gap-2" role="radiogroup">
      {allowGlobal ? (
        <GlobalCard
          selected={value === null}
          onSelect={() => onChange(null)}
          t={t}
        />
      ) : null}
      {WHISPER_MODELS.map((m) => (
        <ModelCard
          key={m.slot}
          model={m}
          selected={value === m.slot}
          recommended={recommendedSlot === m.slot}
          detectedRam={detectedRam}
          onSelect={() => onChange(m.slot)}
          t={t}
        />
      ))}
    </div>
  );
}

function ModelCard({
  model,
  selected,
  recommended,
  detectedRam,
  onSelect,
  t,
}: {
  model: WhisperModelMeta;
  selected: boolean;
  recommended: boolean;
  detectedRam: number;
  onSelect: () => void;
  t: TranslateFn;
}): JSX.Element {
  // RAM warning only when detection actually ran (>0) and falls short.
  const ramShort = detectedRam > 0 && detectedRam < model.minRamGb;
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
      className={
        "text-left rounded-lg border px-3.5 py-3 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 " +
        (selected
          ? "border-brand bg-brand/5"
          : "border-outline bg-surface hover:border-brand/50")
      }
    >
      <div className="flex items-center gap-2">
        <span className="font-medium text-sm text-fg">{t(model.nameKey)}</span>
        {model.german ? (
          <span className="text-[10px] font-semibold uppercase tracking-wide px-1.5 py-0.5 rounded bg-brand/15 text-brand">
            {t("whisper_cards.de_badge")}
          </span>
        ) : null}
        <span className="flex-1" />
        {recommended ? (
          <span className="text-[10px] font-medium px-1.5 py-0.5 rounded-full bg-status-done/15 text-status-done whitespace-nowrap">
            ★ {t("whisper_cards.recommended")}
          </span>
        ) : null}
      </div>
      <p className="text-xs text-fg-muted mt-0.5">{t(model.taglineKey)}</p>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 mt-2">
        <ScoreBar label={t("whisper_cards.tempo")} score={model.speed} />
        <ScoreBar label={t("whisper_cards.accuracy")} score={model.accuracy} />
      </div>
      <div className="flex items-center gap-2 mt-2 text-[11px]">
        <span className={ramShort ? "text-status-error" : "text-fg-faint"}>
          {t("whisper_cards.footprint", {
            size: model.sizeMb,
            ram: model.minRamGb,
          })}
        </span>
        {ramShort ? (
          <span className="text-status-error">
            ·{" "}
            {t("whisper_cards.ram_warning", {
              ram: model.minRamGb,
              have: detectedRam.toFixed(0),
            })}
          </span>
        ) : null}
        <span className="flex-1" />
        <span
          className={
            "font-medium " + (selected ? "text-brand" : "text-fg-faint")
          }
        >
          {selected ? t("whisper_cards.selected") : t("whisper_cards.select")}
        </span>
      </div>
    </button>
  );
}

function GlobalCard({
  selected,
  onSelect,
  t,
}: {
  selected: boolean;
  onSelect: () => void;
  t: TranslateFn;
}): JSX.Element {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onSelect}
      className={
        "text-left rounded-lg border px-3.5 py-2.5 transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 " +
        (selected
          ? "border-brand bg-brand/5"
          : "border-outline bg-surface hover:border-brand/50")
      }
    >
      <div className="flex items-center gap-2">
        <span className="font-medium text-sm text-fg">
          {t("whisper_cards.global")}
        </span>
        <span className="flex-1" />
        <span
          className={
            "text-[11px] font-medium " +
            (selected ? "text-brand" : "text-fg-faint")
          }
        >
          {selected ? t("whisper_cards.selected") : t("whisper_cards.select")}
        </span>
      </div>
      <p className="text-xs text-fg-muted mt-0.5">
        {t("whisper_cards.global_desc")}
      </p>
    </button>
  );
}

/** Five-dot qualitative bar (filled = score). */
function ScoreBar({
  label,
  score,
}: {
  label: string;
  score: number;
}): JSX.Element {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="text-[11px] text-fg-muted">{label}</span>
      <span className="inline-flex gap-0.5" aria-hidden>
        {[1, 2, 3, 4, 5].map((i) => (
          <span
            key={i}
            className={
              "inline-block h-1.5 w-1.5 rounded-full " +
              (i <= score ? "bg-brand" : "bg-outline")
            }
          />
        ))}
      </span>
      <span className="sr-only">{score} / 5</span>
    </span>
  );
}
