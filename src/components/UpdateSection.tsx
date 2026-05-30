// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import Field from "./Field";
import Button from "./Button";
import { ipcGetAppVersion } from "../lib/tauri";
import { useT, type TranslateFn } from "../i18n";

// Self-update only applies to AppImage (Linux) + NSIS (Windows). deb/rpm
// have no updater path — hence the note in `hint`. Download
// only starts on click (large bundle, possibly metered connection).
type Status =
  | "idle"
  | "checking"
  | "uptodate"
  | "available"
  | "downloading"
  | "installing"
  | "error";

function fmtSize(t: TranslateFn, bytes: number): string {
  if (bytes < 1024 * 1024) {
    return t("common.unit.kb", { value: (bytes / 1024).toFixed(0) });
  }
  if (bytes < 1024 * 1024 * 1024) {
    return t("common.unit.mb", { value: (bytes / (1024 * 1024)).toFixed(1) });
  }
  return t("common.unit.gb", {
    value: (bytes / (1024 * 1024 * 1024)).toFixed(2),
  });
}

export default function UpdateSection(): JSX.Element {
  const t = useT();
  const [version, setVersion] = useState("");
  const [status, setStatus] = useState<Status>("idle");
  const [update, setUpdate] = useState<Update | null>(null);
  const [downloaded, setDownloaded] = useState(0);
  const [total, setTotal] = useState<number | null>(null);
  const [errorMsg, setErrorMsg] = useState("");

  useEffect(() => {
    void ipcGetAppVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

  const onCheck = async () => {
    setStatus("checking");
    setErrorMsg("");
    try {
      const found = await check();
      if (found) {
        setUpdate(found);
        setStatus("available");
      } else {
        setStatus("uptodate");
      }
    } catch (e) {
      setErrorMsg(String(e));
      setStatus("error");
    }
  };

  const onInstall = async () => {
    if (!update) return;
    setDownloaded(0);
    setTotal(null);
    setStatus("downloading");
    try {
      let acc = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setTotal(event.data.contentLength ?? null);
        } else if (event.event === "Progress") {
          acc += event.data.chunkLength;
          setDownloaded(acc);
        } else if (event.event === "Finished") {
          setStatus("installing");
        }
      });
      // AppImage is replaced in memory -> restart needed; on Windows
      // the NSIS installer takes over and restarts afterwards.
      await relaunch();
    } catch (e) {
      setErrorMsg(String(e));
      setStatus("error");
    }
  };

  const busy =
    status === "checking" ||
    status === "downloading" ||
    status === "installing";
  const pct =
    total && total > 0 ? Math.round((downloaded / total) * 100) : null;

  return (
    <Field label={t("settings.update.label")} hint={t("settings.update.hint")}>
      <div className="flex flex-col gap-3">
        <div className="text-sm text-fg-muted">
          {t("settings.update.current", { version: version || "—" })}
        </div>

        {status === "available" && update ? (
          <div className="rounded-md border border-brand/40 bg-brand/5 p-3 flex flex-col gap-2">
            <div className="text-sm font-medium text-fg">
              {t("settings.update.available", { version: update.version })}
            </div>
            {update.body ? (
              <pre className="text-xs text-fg-muted whitespace-pre-wrap font-sans max-h-40 overflow-auto">
                {update.body}
              </pre>
            ) : null}
            <Button onClick={() => void onInstall()} className="self-start">
              {t("settings.update.btn.install")}
            </Button>
          </div>
        ) : null}

        {status === "downloading" ? (
          <div className="flex flex-col gap-1 text-xs text-fg-muted">
            <div>
              {total
                ? t("settings.update.downloading", {
                    current: fmtSize(t, downloaded),
                    total: fmtSize(t, total),
                  })
                : t("settings.update.downloading_indeterminate", {
                    current: fmtSize(t, downloaded),
                  })}
            </div>
            {pct !== null ? (
              <div className="h-1.5 bg-elevated rounded-full overflow-hidden">
                <div
                  className="h-full bg-brand transition-all"
                  style={{ width: `${pct}%` }}
                />
              </div>
            ) : null}
          </div>
        ) : null}

        {status === "installing" ? (
          <div className="text-xs text-fg-muted">
            {t("settings.update.installing")}
          </div>
        ) : null}

        {status === "uptodate" ? (
          <div className="text-xs text-status-done">
            {t("settings.update.uptodate")}
          </div>
        ) : null}

        {status === "error" ? (
          <div className="text-xs text-status-error">
            {t("settings.update.error", { message: errorMsg })}
          </div>
        ) : null}

        <Button
          variant="secondary"
          onClick={() => void onCheck()}
          disabled={busy}
          className="self-start"
        >
          {status === "checking"
            ? t("settings.update.btn.checking")
            : t("settings.update.btn.check")}
        </Button>
      </div>
    </Field>
  );
}
