// SPDX-License-Identifier: GPL-3.0-or-later
import type { ButtonHTMLAttributes, ReactNode } from "react";

type Variant =
  | "primary"
  | "secondary"
  | "ghost"
  | "danger"
  | "danger-strong"
  | "tab";
type Size = "md" | "sm";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  children: ReactNode;
}

/*
 * Consistency anchor for every button in the app: fixed height (h-9 /
 * h-7) plus leading-none + whitespace-nowrap keep sizing deterministic,
 * inline-flex centers content, and focus-visible-ring covers keyboard
 * accessibility.
 */
const BASE =
  "inline-flex items-center justify-center whitespace-nowrap leading-none rounded-md font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 focus-visible:ring-offset-2 focus-visible:ring-offset-canvas disabled:cursor-not-allowed";

const VARIANTS: Record<Variant, string> = {
  primary:
    "bg-brand text-brand-contrast hover:bg-brand-hover disabled:bg-elevated disabled:text-fg-faint disabled:hover:bg-elevated",
  secondary:
    "bg-elevated text-fg hover:bg-outline-strong/40 disabled:opacity-50 disabled:hover:bg-elevated",
  ghost:
    "text-fg-muted hover:bg-elevated hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-fg-muted",
  // `danger` — subtle, hover-tint only. For inline actions where the
  // delete button should not shout (e.g. edit/delete rows in tables).
  danger:
    "bg-elevated text-fg-muted hover:bg-status-error/15 hover:text-status-error disabled:opacity-50",
  // `danger-strong` — permanently tinted, for deliberate dangerous
  // actions (factory reset, "delete all"). Visually marked as risky.
  "danger-strong":
    "bg-status-error/15 text-status-error border border-status-error/40 hover:bg-status-error/25 disabled:opacity-50",
  // `tab` — for sidebar navigation. Active state is set via
  // `aria-current`; the background state is driven by the consumer
  // through className.
  tab: "text-fg-muted hover:bg-elevated/60 hover:text-fg aria-[current=page]:bg-elevated aria-[current=page]:text-fg aria-[current=page]:font-medium",
};

const SIZES: Record<Size, string> = {
  md: "h-9 px-4 text-sm gap-2",
  sm: "h-7 px-2.5 text-xs gap-1.5",
};

export default function Button({
  variant = "primary",
  size = "md",
  className = "",
  children,
  type = "button",
  ...rest
}: ButtonProps): JSX.Element {
  const cn = `${BASE} ${VARIANTS[variant]} ${SIZES[size]} ${className}`.trim();
  return (
    <button type={type} className={cn} {...rest}>
      {children}
    </button>
  );
}
