// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect } from "react";
import { shownWindows, type UsageLevel } from "../lib/chatgptUsage";
import { useChatGptUsage } from "../lib/useChatGptUsage";
import { formatDate, formatNumber } from "../i18n/format";
import { useLocale, useT } from "../i18n";

const BAR: Record<UsageLevel, string> = {
  ok: "bg-brand",
  warn: "bg-status-processing",
  reached: "bg-status-error",
};

const TEXT: Record<UsageLevel, string> = {
  ok: "text-fg-muted",
  warn: "text-status-processing",
  reached: "text-status-error",
};

/**
 * Usage meters of the ChatGPT plan inside the connected account card.
 * Fetches once on mount (the backend throttles to once a minute); later
 * updates arrive for free with every ChatGPT post-processing call.
 */
export default function ChatGptUsageMeters(): JSX.Element {
  const t = useT();
  const locale = useLocale();
  const { usage, refresh, refreshing, error } = useChatGptUsage();

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const now = Date.now() / 1000;
  const windows = shownWindows(usage, now);
  // Absolute local times, so nothing has to tick: "16:40" today,
  // "Mon 09:12" otherwise.
  const time = (sec: number) => {
    const d = new Date(sec * 1000);
    const today = new Date().toDateString() === d.toDateString();
    return formatDate(
      d,
      locale,
      today
        ? { hour: "2-digit", minute: "2-digit" }
        : { weekday: "short", hour: "2-digit", minute: "2-digit" },
    );
  };

  return (
    <div className="flex flex-col gap-2 border-t border-outline pt-3">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium text-fg-muted">
          {t("chatgpt.usage.title")}
        </span>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={refreshing}
          className="text-xs text-brand hover:underline disabled:opacity-50"
        >
          {t("chatgpt.usage.refresh")}
        </button>
      </div>

      {usage === null ? (
        <div className="text-xs text-fg-faint" title={error ?? undefined}>
          {error ? t("chatgpt.usage.unavailable") : t("chatgpt.usage.loading")}
        </div>
      ) : windows.length === 0 ? (
        <div className="text-xs text-fg-faint">{t("chatgpt.usage.none")}</div>
      ) : (
        windows.map(({ kind, window, level }) => (
          <div key={kind} className="flex items-center gap-3 text-xs">
            <span className="w-20 shrink-0 text-fg-muted">
              {kind === "other"
                ? t("chatgpt.usage.window.other", {
                    hours: Math.round((window.window_minutes ?? 0) / 60),
                  })
                : t(`chatgpt.usage.window.${kind}`)}
            </span>
            <div className="h-1.5 flex-1 bg-elevated rounded-full overflow-hidden">
              <div
                className={`h-full ${BAR[level]}`}
                style={{ width: `${Math.min(100, window.used_percent)}%` }}
              />
            </div>
            <span className={`shrink-0 ${TEXT[level]}`}>
              {level === "reached"
                ? t("chatgpt.usage.reached")
                : t("chatgpt.usage.used", {
                    percent: formatNumber(window.used_percent / 100, locale, {
                      style: "percent",
                      maximumFractionDigits: 0,
                    }),
                  })}
              {window.resets_at
                ? ` · ${t("chatgpt.usage.resets", { time: time(window.resets_at) })}`
                : ""}
            </span>
          </div>
        ))
      )}

      {usage ? (
        <p className="text-[11px] text-fg-faint">
          {error ? `${t("chatgpt.usage.stale")} ` : ""}
          {t("chatgpt.usage.as_of", { time: time(usage.fetched_at) })}{" "}
          {t("chatgpt.usage.note")}
        </p>
      ) : null}
    </div>
  );
}
