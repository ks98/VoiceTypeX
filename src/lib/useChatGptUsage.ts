// SPDX-License-Identifier: GPL-3.0-or-later
import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { EVENTS } from "./events";
import { listenAll } from "./tauriListen";
import {
  ipcGetChatGptUsage,
  ipcRefreshChatGptUsage,
  type ChatGptUsage,
} from "./tauri";

interface ChatGptUsageState {
  usage: ChatGptUsage | null;
  refresh: () => Promise<void>;
  refreshing: boolean;
  /** Last refresh failure (English backend text); the snapshot stays. */
  error: string | null;
}

/**
 * ChatGPT usage snapshot: the cached value on mount, then every update the
 * backend pushes (Settings fetch or post-processing headers).
 */
export function useChatGptUsage(): ChatGptUsageState {
  const [usage, setUsage] = useState<ChatGptUsage | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void ipcGetChatGptUsage()
      .then(setUsage)
      .catch(() => undefined);
    return listenAll([
      listen<ChatGptUsage | null>(EVENTS.CHATGPT_USAGE, (event) =>
        setUsage(event.payload),
      ),
    ]);
  }, []);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    setError(null);
    try {
      setUsage(await ipcRefreshChatGptUsage());
    } catch (e) {
      setError(String(e));
    } finally {
      setRefreshing(false);
    }
  }, []);

  return { usage, refresh, refreshing, error };
}
