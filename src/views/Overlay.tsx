// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useRef, useState } from "react";
import { listen, emit } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listenAll } from "../lib/tauriListen";
import { EVENTS } from "../lib/events";
import { useLocale, useT } from "../i18n";
import { formatDate, formatNumber } from "../i18n/format";
import { overlayHint } from "../lib/chatgptUsage";
import { useChatGptUsage } from "../lib/useChatGptUsage";
import { ipcGetEffectiveMenuHotkey, ipcGetSettings } from "../lib/tauri";
import {
  errorStage,
  hotkeyLabel,
  silenceHintDue,
  type EngineStatus,
  type Phase,
} from "../lib/overlayModel";
import OverlayCard from "../components/overlay/OverlayCard";
import type {
  LevelListener,
  LevelSubscribe,
} from "../components/overlay/LevelWaveform";

type StatePayload = { state: Phase; error?: string };
type PartialTranscriptPayload = { text: string };
type AudioLevelPayload = { level: number };

/** Phases with a running clock. */
const TIMED: readonly Phase[] = ["recording", "transcribing", "postprocessing"];
const TICK_MS = 250;

/**
 * Live overlay window — container of the overlay card: subscribes to the
 * pipeline events and derives what the pure `OverlayCard` renders.
 * Visibility is driven by the backend.
 *
 * The phase default is "recording" because the backend only makes the
 * window visible when the pipeline transitions into recording, so the
 * first visible frame never shows an empty card.
 *
 * `app://partial-transcript` (local Whisper streaming) carries the live
 * transcript; it is cleared when the phase leaves recording.
 * `app://audio-level` feeds the waveform at 25 Hz through a small fan-out
 * instead of React state, so level updates never re-render the tree.
 */
export default function Overlay(): JSX.Element {
  const t = useT();
  const locale = useLocale();
  const { usage } = useChatGptUsage();
  const [phase, setPhase] = useState<Phase>("recording");
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [failedIn, setFailedIn] = useState<Phase | null>(null);
  const [partial, setPartial] = useState("");
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [session, setSession] = useState(0);
  const [hotkey, setHotkey] = useState<string | null>(null);
  const [phaseStart, setPhaseStart] = useState(() => Date.now());
  const [now, setNow] = useState(() => Date.now());
  const [silent, setSilent] = useState(false);

  const lastPhase = useRef<Phase | null>(null);
  const maxLevel = useRef(0);
  const levelListeners = useRef(new Set<LevelListener>());

  const subscribeLevels = useCallback<LevelSubscribe>((listener) => {
    levelListeners.current.add(listener);
    return () => {
      levelListeners.current.delete(listener);
    };
  }, []);

  useEffect(() => {
    return listenAll([
      listen<StatePayload>(EVENTS.STATE, (event) => {
        const next = event.payload.state;
        const previous = lastPhase.current;
        lastPhase.current = next;
        const at = Date.now();
        setPhase(next);
        setErrorMsg(event.payload.error ?? null);
        setPhaseStart(at);
        setNow(at);
        if (next === "error") setFailedIn(previous);
        if (next !== "recording") {
          setPartial("");
        } else if (previous !== "recording") {
          setSession((s) => s + 1);
          setSilent(false);
          maxLevel.current = 0;
          // Re-read per recording: the hotkey may have changed meanwhile.
          void Promise.all([ipcGetEffectiveMenuHotkey(), ipcGetSettings()])
            .then(([effective, settings]) =>
              setHotkey(hotkeyLabel(effective ?? settings.menu_hotkey)),
            )
            .catch(() => setHotkey(null));
        }
      }),
      listen<PartialTranscriptPayload>(EVENTS.PARTIAL_TRANSCRIPT, (event) => {
        setPartial(event.payload.text ?? "");
      }),
      // Engine status (issue #8): emitted by the backend when a mode becomes
      // active (recording start). Stays until the next recording overwrites it.
      listen<EngineStatus>(EVENTS.ACTIVE_ENGINE, (event) => {
        setEngine(event.payload);
      }),
      listen<AudioLevelPayload>(EVENTS.AUDIO_LEVEL, (event) => {
        const level = event.payload.level;
        if (level > maxLevel.current) maxLevel.current = level;
        levelListeners.current.forEach((listener) => listener(level));
      }),
    ]);
  }, []);

  useEffect(() => {
    if (!TIMED.includes(phase)) return;
    const id = window.setInterval(() => {
      const at = Date.now();
      setNow(at);
      if (phase === "recording") {
        setSilent(silenceHintDue(at - phaseStart, maxLevel.current));
      }
    }, TICK_MS);
    return () => window.clearInterval(id);
  }, [phase, phaseStart]);

  // ChatGPT usage hint (≥ 80 %), only while a mode that uses ChatGPT runs.
  const usesChatGpt =
    engine?.stt.provider === "chatgpt" || engine?.llm?.provider === "chatgpt";
  const hint = usesChatGpt ? overlayHint(usage, now / 1000) : null;
  const hintText = !hint
    ? null
    : hint.level === "reached"
      ? hint.window.resets_at
        ? t("overlay.usage.reached", {
            time: formatDate(hint.window.resets_at * 1000, locale, {
              weekday: "short",
              hour: "2-digit",
              minute: "2-digit",
            }),
          })
        : t("chatgpt.usage.reached")
      : t("overlay.usage.warn", {
          window:
            hint.kind === "other"
              ? t("chatgpt.usage.window.other", {
                  hours: Math.round((hint.window.window_minutes ?? 0) / 60),
                })
              : t(`chatgpt.usage.window.${hint.kind}`),
          percent: formatNumber(hint.window.used_percent / 100, locale, {
            style: "percent",
            maximumFractionDigits: 0,
          }),
        });

  return (
    <div className="h-screen w-screen overflow-hidden p-2 select-none pointer-events-none">
      <OverlayCard
        phase={phase}
        session={session}
        engine={engine}
        partial={partial}
        elapsedMs={now - phaseStart}
        silent={silent}
        hotkey={hotkey}
        usageHint={
          hint && hintText
            ? {
                text: hintText,
                level: hint.level === "reached" ? "reached" : "warn",
              }
            : null
        }
        error={
          phase === "error"
            ? { message: errorMsg, stage: errorStage(failedIn) }
            : null
        }
        levels={subscribeLevels}
        onErrorClick={openLogsInMainWindow}
      />
    </div>
  );
}

/**
 * When the user clicks the error card, we bring the main window to
 * the front and signal "show logs" via an event. App.tsx listens to
 * `app://focus-logs` and switches to the Logs tab.
 */
async function openLogsInMainWindow(): Promise<void> {
  try {
    const main = await WebviewWindow.getByLabel("main");
    if (main) {
      await main.show();
      await main.setFocus();
    }
    await emit(EVENTS.FOCUS_LOGS);
  } catch {
    // The window API can fail on restrictive capabilities — in that
    // case the user still sees the error message in the overlay (with
    // the full text as tooltip), which is tolerable.
  }
}
