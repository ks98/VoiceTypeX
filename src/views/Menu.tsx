// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef, useState } from "react";
import { useModesStore, useSettingsStore } from "../store";
import { ipcCancelMenu, ipcStartRecording } from "../lib/tauri";

/**
 * Modus-Auswahl-Menü.
 *
 * Eigenes Tauri-Window (`label: "menu"`), wird vom Backend per
 * `menu.show() + set_focus()` sichtbar gemacht, sobald der globale
 * Menue-Hotkey im Idle-State gedrueckt wurde.
 *
 * - `↑` / `↓` navigieren den Cursor, `Home` / `End` springen an die Enden.
 * - `Enter` ruft `start_recording` mit dem ausgewaehlten Modus — das
 *   Backend versteckt das Menue und zeigt das Status-Overlay.
 * - `Esc` ruft `cancel_menu` — Backend versteckt das Menue ohne State-
 *   Wechsel.
 *
 * Der Cursor steht initial auf `Settings.last_selected_mode_id`, sodass
 * die haeufigste Aktion ein einzelner Enter-Druck ist.
 */
export default function Menu() {
  const modes = useModesStore((s) => s.modes);
  const loadModes = useModesStore((s) => s.load);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);

  const [cursor, setCursor] = useState(0);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const itemRefs = useRef<Array<HTMLLIElement | null>>([]);

  // Beim Mount: Stores hydrieren + Fokus ans Root-Element.
  useEffect(() => {
    void loadModes();
    void loadSettings();
    rootRef.current?.focus();
  }, [loadModes, loadSettings]);

  // Sobald Modi + Settings geladen sind: Cursor auf zuletzt gewaehlten
  // Modus setzen.
  useEffect(() => {
    if (modes.length === 0) return;
    const lastId = settings?.last_selected_mode_id ?? null;
    const idx = lastId ? modes.findIndex((m) => m.id === lastId) : -1;
    setCursor(idx >= 0 ? idx : 0);
  }, [modes, settings?.last_selected_mode_id]);

  // Bei Cursor-Bewegung den aktiven Eintrag in den sichtbaren Bereich
  // scrollen — bei langen Modus-Listen sonst nicht erreichbar.
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
      <div className="h-full w-full rounded-lg bg-black/85 backdrop-blur-md border border-white/15 shadow-2xl flex flex-col overflow-hidden">
        <div className="px-4 py-2 border-b border-white/10 flex items-center justify-between shrink-0">
          <span className="text-white/90 text-xs font-semibold tracking-wide">
            Modus waehlen
          </span>
          <span className="text-white/40 text-[10px] font-mono">
            ↑ ↓ Enter · Esc
          </span>
        </div>
        {modes.length === 0 ? (
          <div className="flex-1 flex items-center justify-center px-4 py-3">
            <p className="text-white/70 text-sm italic">
              Keine Modi geladen — leg eine TOML unter modes/ an.
            </p>
          </div>
        ) : (
          <ul className="flex-1 overflow-y-auto py-1">
            {modes.map((m, i) => {
              const active = i === cursor;
              return (
                <li
                  key={m.id}
                  ref={(el) => {
                    itemRefs.current[i] = el;
                  }}
                  className={`px-4 py-1.5 flex flex-col gap-0.5 ${
                    active ? "bg-white/15" : ""
                  }`}
                >
                  <span
                    className={`text-sm ${
                      active ? "text-white font-medium" : "text-white/85"
                    }`}
                  >
                    {m.name}
                  </span>
                  {m.description ? (
                    <span className="text-[11px] text-white/40 truncate">
                      {m.description}
                    </span>
                  ) : null}
                </li>
              );
            })}
          </ul>
        )}
        {errorMsg ? (
          <div className="px-4 py-1.5 border-t border-red-700/40 bg-red-900/30 text-xs text-red-200 truncate shrink-0">
            {errorMsg}
          </div>
        ) : null}
      </div>
    </div>
  );
}
