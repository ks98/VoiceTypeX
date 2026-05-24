// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useMemo, useRef, useState } from "react";
import { useModesStore, useSettingsStore } from "../store";
import { ipcCancelMenu, ipcReloadModes, ipcStartRecording } from "../lib/tauri";
import type { Mode } from "../lib/types";
import Banner from "../components/Banner";
import { useT, type TranslateFn } from "../i18n";

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
  const t = useT();
  const modes = useModesStore((s) => s.modes);
  const loadModes = useModesStore((s) => s.load);
  const settings = useSettingsStore((s) => s.settings);
  const loadSettings = useSettingsStore((s) => s.load);

  const [cursor, setCursor] = useState(0);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  // C9: Wayland-Compositors fokussieren neu eingeblendete Windows nicht
  // immer automatisch (z.B. sway ohne `focus_on_window_activation`).
  // Wenn document.hasFocus() beim Mount false ist, blenden wir einen
  // dezenten Hinweis ein — User klickt einmal ins Fenster, dann gehen
  // die Tasten. Erster Keydown blendet den Hinweis wieder aus.
  const [focusWarning, setFocusWarning] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const itemRefs = useRef<Array<HTMLLIElement | null>>([]);

  useEffect(() => {
    void loadModes();
    void loadSettings();
    rootRef.current?.focus();
  }, [loadModes, loadSettings]);

  useEffect(() => {
    // Initial-Check + verzoegerter Re-Check: Compositors brauchen
    // manchmal einen Tick, bis das Focus-Event durchlaeuft.
    const check = () => {
      if (!document.hasFocus()) {
        setFocusWarning(true);
      }
    };
    check();
    const t = window.setTimeout(check, 150);
    return () => window.clearTimeout(t);
  }, []);

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
      // Jeder Tastendruck beweist Focus — Warnung sofort weg.
      if (focusWarning) setFocusWarning(false);

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
    [modes, cursor, focusWarning],
  );

  const lastSelectedId = settings?.last_selected_mode_id ?? null;

  return (
    <div
      ref={rootRef}
      tabIndex={-1}
      onKeyDown={(e) => void onKeyDown(e)}
      className="h-screen w-screen overflow-hidden p-2 select-none outline-none"
    >
      <div className="h-full w-full rounded-lg vtx-glass shadow-2xl flex flex-col overflow-hidden">
        <div className="px-4 py-2.5 border-b border-fg/10 flex items-center justify-between shrink-0">
          <span className="text-fg text-xs font-semibold tracking-wide uppercase">
            {t("menu.title")}
          </span>
          <span className="text-fg-faint text-xxs font-mono">
            {t("menu.kbd_hint")}
          </span>
        </div>
        {focusWarning ? (
          <div className="px-3 py-2 shrink-0">
            <Banner tone="warning" dense>
              <span className="text-xxs">{t("menu.focus_warning")}</span>
            </Banner>
          </div>
        ) : null}
        {modes.length === 0 ? (
          <EmptyState />
        ) : (
          <ul className="flex-1 overflow-y-auto py-1">
            {modes.map((m, i) => (
              <ModeRow
                key={m.id}
                t={t}
                mode={m}
                active={i === cursor}
                lastUsed={m.id === lastSelectedId}
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

function EmptyState(): JSX.Element {
  const t = useT();
  const loadModes = useModesStore((s) => s.load);
  const [busy, setBusy] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  // `ipcReloadModes` liest die Modi-TOMLs vom Disk neu — wenn dort keine
  // liegen, kommt eine leere Liste zurueck. Der Bootstrap-Pfad, der die
  // 6 Default-Modi schreibt, laeuft nur beim App-Start (verifiziert
  // gegen das Backend). Darum: Versuch via reload — wenn das nichts
  // bringt, freundlicher Hinweis auf Neustart.
  const onReset = async () => {
    setBusy(true);
    setHint(null);
    try {
      const result = await ipcReloadModes();
      await loadModes();
      if (result.length === 0) {
        setHint(t("menu.empty.hint_after_load"));
      }
    } catch (e) {
      setHint(t("menu.empty.hint_error", { message: String(e) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex-1 flex flex-col items-center justify-center gap-3 px-6 py-4 text-center">
      <p className="text-fg-muted text-sm">{t("menu.empty.intro")}</p>
      <button
        type="button"
        onClick={() => void onReset()}
        disabled={busy}
        className="px-3 py-1.5 rounded-md bg-brand text-brand-contrast text-xs font-medium hover:bg-brand-hover disabled:opacity-50 transition-colors"
      >
        {busy ? t("menu.empty.button_loading") : t("menu.empty.button_load")}
      </button>
      {hint ? <p className="text-xxs text-fg-faint">{hint}</p> : null}
    </div>
  );
}

function ModeRow({
  t,
  mode,
  active,
  lastUsed,
  refCb,
}: {
  t: TranslateFn;
  mode: Mode;
  active: boolean;
  lastUsed: boolean;
  refCb: (el: HTMLLIElement | null) => void;
}): JSX.Element {
  const offline = isOfflineMode(mode);
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
          "shrink-0 inline-flex items-center justify-center h-7 w-7"
        }
        aria-label={offline ? t("menu.aria.offline") : t("menu.aria.online")}
        title={
          offline ? t("menu.title_offline") : t("menu.title_online")
        }
      >
        <span
          className={
            "h-2.5 w-2.5 rounded-full " +
            (offline ? "bg-status-done" : "bg-brand")
          }
          aria-hidden
        />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span
            className={
              "text-sm truncate " +
              (active ? "text-fg font-medium" : "text-fg")
            }
          >
            {mode.name}
          </span>
          {lastUsed ? (
            <span
              className="shrink-0 inline-block h-[3px] w-[3px] rounded-full bg-brand"
              aria-label={t("menu.aria.last_used")}
              title={t("menu.title_last_used")}
            />
          ) : null}
        </div>
        {mode.description ? (
          <div className="text-xxs text-fg-faint truncate">
            {mode.description}
          </div>
        ) : null}
      </div>
    </li>
  );
}

/**
 * Ein Modus ist "vollstaendig offline", wenn weder STT noch
 * Post-Processing eine Cloud-Komponente brauchen. processing="none"
 * zaehlt als kein Cloud-Bedarf — der Post-Processing-Step entfaellt
 * dann ganz.
 */
function isOfflineMode(mode: Mode): boolean {
  return (
    mode.transcription === "local" &&
    (mode.processing === "none" || mode.processing === "local")
  );
}
