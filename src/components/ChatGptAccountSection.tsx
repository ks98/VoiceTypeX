// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import Banner from "./Banner";
import Button from "./Button";
import Input from "./Input";
import Loading from "./Loading";
import { EVENTS } from "../lib/events";
import { listenAll } from "../lib/tauriListen";
import {
  ipcChatGptLoginCancel,
  ipcChatGptLoginCompleteManual,
  ipcChatGptLoginStart,
  ipcChatGptLogout,
  ipcGetChatGptStatus,
  type ChatGptStatus,
} from "../lib/tauri";
import { useT } from "../i18n";

// The backend owns the sign-in state and pushes every change as an event;
// this component only keeps UI-local extras (consent tick, paste draft).
// Leaving the view does not cancel a pending sign-in — the backend times
// it out on its own.
export default function ChatGptAccountSection(): JSX.Element {
  const t = useT();
  const [status, setStatus] = useState<ChatGptStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [ack, setAck] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pasteDraft, setPasteDraft] = useState("");
  const [copied, setCopied] = useState(false);

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

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setActionError(null);
    try {
      await action();
    } catch (e) {
      setActionError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onConnect = () =>
    void run(async () => {
      setCopied(false);
      setPasteDraft("");
      setStatus(await ipcChatGptLoginStart());
    });

  const onCompleteManual = () =>
    void run(async () => {
      setStatus(await ipcChatGptLoginCompleteManual(pasteDraft));
      setPasteDraft("");
    });

  const onCopyLink = () => {
    if (!status?.auth_url) return;
    void navigator.clipboard
      .writeText(status.auth_url)
      .then(() => setCopied(true))
      .catch((e) => setActionError(String(e)));
  };

  const onDisconnect = () => {
    if (window.confirm(t("chatgpt.disconnect.confirm"))) {
      void run(ipcChatGptLogout);
    }
  };

  if (!status && !loadError) {
    return <Loading label={t("chatgpt.loading")} />;
  }

  const plan = status?.plan
    ? status.plan.charAt(0).toUpperCase() + status.plan.slice(1)
    : null;

  return (
    <div className="flex flex-col gap-3">
      <div>
        <h2 className="text-lg font-semibold text-fg">{t("chatgpt.title")}</h2>
        <p className="text-xs text-fg-faint mt-1">{t("chatgpt.intro")}</p>
      </div>

      {loadError ? (
        <Banner tone="error">
          {t("chatgpt.load_error", { message: loadError })}
        </Banner>
      ) : null}
      {status?.error ? (
        <Banner tone="error">
          {t("chatgpt.error", { message: status.error })}
        </Banner>
      ) : null}
      {actionError ? <Banner tone="error">{actionError}</Banner> : null}

      {status?.state === "disconnected" ? (
        <div className="flex flex-col gap-3">
          <Banner tone="warning">
            <div className="flex flex-col gap-1">
              <span className="font-medium">{t("chatgpt.warning.title")}</span>
              <span>{t("chatgpt.warning.body")}</span>
            </div>
          </Banner>
          <label className="flex items-center gap-2 text-sm text-fg">
            <input
              type="checkbox"
              checked={ack}
              onChange={(e) => setAck(e.target.checked)}
            />
            {t("chatgpt.ack.label")}
          </label>
          <Button
            onClick={onConnect}
            disabled={!ack || busy}
            className="self-start"
          >
            {t("chatgpt.btn.connect")}
          </Button>
        </div>
      ) : null}

      {status?.state === "pending" ? (
        <div className="flex flex-col gap-3 border border-outline rounded-md p-4 bg-surface">
          <div>
            <div className="text-sm font-medium text-fg">
              {t("chatgpt.pending.title")}
            </div>
            <p className="text-xs text-fg-muted mt-1">
              {t("chatgpt.pending.body")}
            </p>
          </div>
          <div className="flex gap-2">
            <Button variant="secondary" size="sm" onClick={onCopyLink}>
              {copied ? t("chatgpt.btn.copied") : t("chatgpt.btn.copy_link")}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => void run(ipcChatGptLoginCancel)}
            >
              {t("common.cancel")}
            </Button>
          </div>
          <details className="text-xs text-fg-muted">
            <summary className="cursor-pointer">
              {t("chatgpt.paste.summary")}
            </summary>
            <div className="flex gap-2 mt-2">
              <Input
                value={pasteDraft}
                onChange={(e) => setPasteDraft(e.target.value)}
                placeholder="http://127.0.0.1:1455/auth/callback?code=…"
                spellCheck={false}
              />
              <Button
                size="sm"
                disabled={busy || pasteDraft.trim() === ""}
                onClick={onCompleteManual}
              >
                {t("chatgpt.paste.submit")}
              </Button>
            </div>
          </details>
        </div>
      ) : null}

      {status?.state === "connected" ? (
        <div className="flex items-center justify-between gap-3 border border-outline rounded-md p-4 bg-surface">
          <div className="flex flex-col gap-0.5 text-sm">
            <span className="font-medium text-status-done">
              {t("chatgpt.status.connected")}
            </span>
            <span className="text-fg-muted">
              {[status.email, plan ? `ChatGPT ${plan}` : null]
                .filter(Boolean)
                .join(" · ")}
            </span>
          </div>
          <Button
            variant="danger"
            size="sm"
            disabled={busy}
            onClick={onDisconnect}
          >
            {t("chatgpt.btn.disconnect")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}
