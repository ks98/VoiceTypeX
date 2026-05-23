// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

type Tone = "error" | "warning" | "success" | "info";

interface BannerProps {
  /** Bestimmt Farbe + ARIA-Role (error → alert). */
  tone: Tone;
  /** Dichtere Padding-Variante fuer Inline-Use in Listen. */
  dense?: boolean;
  /** Optionaler Block am rechten Rand (z.B. Action-Button). */
  action?: ReactNode;
  className?: string;
  children: ReactNode;
}

/**
 * Status-Banner — zentralisiert die `rounded-md bg-status-X/10 border
 * border-status-X/40 px-3 py-2`-Trias, die in der App 9× auftauchte.
 *
 * Live-Region-Verhalten: `tone="error"` → `role="alert"`, sonst
 * `role="status"`. Screenreader bekommen damit den Inhalt sofort
 * gemeldet (statt erst beim naechsten Tab-Stop).
 */
export default function Banner({
  tone,
  dense,
  action,
  className = "",
  children,
}: BannerProps): JSX.Element {
  const color = TONE_CLASSES[tone];
  const padding = dense ? "px-3 py-1.5" : "px-3 py-2";
  const role = tone === "error" ? "alert" : "status";
  return (
    <div
      role={role}
      aria-live={tone === "error" ? "assertive" : "polite"}
      className={`rounded-md border ${color.bg} ${color.border} ${padding} text-sm ${color.text} flex items-start gap-3 ${className}`}
    >
      <div className="flex-1 min-w-0">{children}</div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}

const TONE_CLASSES: Record<
  Tone,
  { bg: string; border: string; text: string }
> = {
  error: {
    bg: "bg-status-error/10",
    border: "border-status-error/40",
    text: "text-status-error",
  },
  warning: {
    bg: "bg-status-processing/10",
    border: "border-status-processing/40",
    text: "text-status-processing",
  },
  success: {
    bg: "bg-status-done/10",
    border: "border-status-done/40",
    text: "text-status-done",
  },
  info: {
    bg: "bg-brand/10",
    border: "border-brand/40",
    text: "text-brand",
  },
};
