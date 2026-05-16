// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef, useState } from "react";
import { useModesStore, useSettingsStore } from "../store";
import { ipcCancelMenu, ipcStartRecording } from "../lib/tauri";
import type { Mode } from "../lib/types";

/**
 * Modus-Auswahl-Menü.
 *
 * Eigenes Tauri-Window (label: "menu"), wird vom Backend per
 * menu.show() + set_focus() sichtbar gemacht, sobald der globale
 * Menue-Hotkey im Idle-State gedrückt wurde.
 *
 * - ↑ / ↓ navigieren den Cursor, Home / End springen an die Enden.
 * - Enter ruft start_recording mit dem ausgewählten Modus.
 * - Esc ruft cancel_menu — Backend versteckt das Menü ohne State-Wechsel.
 *
 * Der Cursor steht initial auf Settings.last_selected_mode_id, sodass
 * die häufigste Aktion ein einzelner Enter-Druck ist.
 */
export default function Menu(): JSX.Element {
  const modes = useModesStore((s) => s.modes);
  const loadModes = useModesStore((s) => s.load);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);

  const [cursor, setCursor] = useState(0);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const itemRefs = useRef<Array<HTMLLIElement | null>>([]);

  useEffect(() => {
    void loadModes();
    void loadSettings();
    rootRef.current?.focus();
  }, [loadModes, loadSettings]);

  useEffect(() => {
    if (modes.length === 0) return;
    const lastId = settings?.last_selected_mode_id ?? null;
    const idx = lastId ? modes.findIndex((m) => m.id === lastId) : -1;
    setCursor(idx >= 0 ? idx : 0);
  }, [modes, settings?.last_selected_mode_id]);

  useEffect(() => {
    itemRefs.current[cursor]?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const onKeyDown = useMemo(
    () => async (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (modes.length === 0) return;

      if (event.key === "ArrowDown") {
        event.preventDefault();
        setCursor((c) => (c + 1) % modes.length);
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setCursor((c) => (c - 1 + modes.length) % modes.length);
      } else if (event.key === "Home") {
        event.preventDefault();
        setCursor(0);
      } else if (event.key === "End") {
        event.preventDefault();
        setCursor(modes.length - 1);
      } else if (event.key === "Enter") {
        event.preventDefault();
        const mode = modes[cursor];
        if (!mode) return;
        try {
          await ipcStartRecording(mode.id);
        } catch (e) {
          setErrorMsg(String(e));
        }
      } else if (event.key === "Escape") {
        event.preventDefault();
        try {
          await ipcCancelMenu();
        } catch {
          // ignorieren — Window wird im Zweifel beim nächsten Idle versteckt
        }
      }
    },
    [modes, cursor],
  );

  return (
    <div
      ref={rootRef}
      tabIndex={-1}
      onKeyDown={(e) => void onKeyDown(e)}
      className="h-screen w-screen overflow-hidden p-2 select-none outline-none"
    >
      <div className="h-full w-full rounded-lg bg-canvas/85 backdrop-blur-xl border border-fg/10 shadow-2xl flex flex-col overflow-hidden">
        <div className="px-4 py-2.5 border-b border-fg/10 flex items-center justify-between shrink-0">
          <span className="text-fg text-xs font-semibold tracking-wide uppercase">
            Modus wählen
          </span>
          <span className="text-fg-faint text-[10px] font-mono">
            ↑ ↓ Enter · Esc
          </span>
        </div>
        {modes.length === 0 ? (
          <div className="flex-1 flex items-center justify-center px-4 py-3">
            <p className="text-fg-muted text-sm italic">
              Keine Modi geladen — leg eine TOML unter modes/ an.
            </p>
          </div>
        ) : (
          <ul className="flex-1 overflow-y-auto py-1">
            {modes.map((m, i) => (
              <ModeRow
                key={m.id}
                mode={m}
                active={i === cursor}
                refCb={(el) => {
                  itemRefs.current[i] = el;
                }}
              />
            ))}
          </ul>
        )}
        {errorMsg ? (
          <div className="px-4 py-1.5 border-t border-status-error/40 bg-status-error/10 text-xs text-status-error truncate shrink-0">
            {errorMsg}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function ModeRow({
  mode,
  active,
  refCb,
}: {
  mode: Mode;
  active: boolean;
  refCb: (el: HTMLLIElement | null) => void;
}): JSX.Element {
  return (
    <li
      ref={refCb}
      className={
        "relative px-4 py-2 flex items-center gap-3 transition-colors " +
        (active ? "bg-elevated" : "")
      }
    >
      <span
        className={
          "absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-r " +
          (active ? "bg-brand" : "bg-transparent")
        }
        aria-hidden
      />
      <span
        className={
          "shrink-0 inline-flex items-center justify-center h-7 w-7 rounded-md text-[10px] font-mono font-semibold tracking-wide " +
          (active
            ? "bg-brand text-brand-contrast"
            : "bg-elevated text-fg-muted")
        }
        aria-hidden
      >
        {initialsFor(mode)}
      </span>
      <div className="min-w-0 flex-1">
        <div
          className={
            "text-sm truncate " +
            (active ? "text-fg font-medium" : "text-fg")
          }
        >
          {mode.name}
        </div>
        {mode.description ? (
          <div className="text-[11px] text-fg-faint truncate">
            {mode.description}
          </div>
        ) : null}
      </div>
    </li>
  );
}

function initialsFor(mode: Mode): string {
  const parts = mode.name
    .split(/[\s/\-_]+/)
    .filter((w) => w.length > 0)
    .slice(0, 2);
  if (parts.length === 0) {
    return mode.id.slice(0, 2).toUpperCase();
  }
  return parts.map((w) => w[0]?.toUpperCase() ?? "").join("") || "·";
}
