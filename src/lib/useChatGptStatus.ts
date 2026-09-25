// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { EVENTS } from "./events";
import { listenAll } from "./tauriListen";
import { ipcGetChatGptStatus, type ChatGptStatus } from "./tauri";

interface ChatGptStatusState {
  /** `null` until the first answer from the backend. */
  status: ChatGptStatus | null;
  setStatus: (status: ChatGptStatus) => void;
  loadError: string | null;
}

/** ChatGPT account status, kept current via the backend's status event. */
export function useChatGptStatus(): ChatGptStatusState {
  const [status, setStatus] = useState<ChatGptStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    void ipcGetChatGptStatus()
      .then(setStatus)
      .catch((e) => setLoadError(String(e)));
    return listenAll([
      listen<ChatGptStatus>(EVENTS.CHATGPT_STATUS, (event) =>
        setStatus(event.payload),
      ),
    ]);
  }, []);

  return { status, setStatus, loadError };
}
