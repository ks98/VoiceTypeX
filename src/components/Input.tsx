// SPDX-License-Identifier: GPL-3.0-or-later
import { forwardRef } from "react";
import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";

type Density = "default" | "compact";

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  /** "default" (px-3 py-2, for Settings/Wizard) | "compact"
   * (px-2 py-1.5, for the modal editor). */
  density?: Density;
  /** Additional visual marker for validation errors. */
  invalid?: boolean;
}

interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  density?: Density;
  invalid?: boolean;
}

const DENSITY: Record<Density, string> = {
  default: "px-3 py-2",
  compact: "px-2 py-1.5",
};

const BASE =
  "bg-surface border rounded-md text-sm w-full text-fg placeholder:text-fg-faint focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/40 focus-visible:ring-offset-1 focus-visible:ring-offset-canvas focus:border-brand transition-colors disabled:opacity-50";

/**
 * Consolidated input element. Replaces the 3× duplicated `inputCls`
 * pattern from Settings.tsx, OnboardingWizard.tsx and ModeEditor.tsx
 * (with subtly different paddings).
 *
 * Density:
 * - `default` — for Settings, Onboarding, normal forms.
 * - `compact` — for the modal editor with many fields.
 *
 * `invalid` renders a red border + red focus ring; for full a11y also
 * add `aria-invalid` and a visible help text (e.g. via Field hint).
 */
const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { density = "default", invalid, className = "", ...rest },
  ref,
) {
  const borderColor = invalid
    ? "border-status-error focus:border-status-error focus-visible:ring-status-error/40"
    : "border-outline";
  return (
    <input
      ref={ref}
      className={`${BASE} ${DENSITY[density]} ${borderColor} ${className}`}
      aria-invalid={invalid || undefined}
      {...rest}
    />
  );
});

export default Input;

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(
  function Textarea(
    { density = "default", invalid, className = "", ...rest },
    ref,
  ) {
    const borderColor = invalid
      ? "border-status-error focus:border-status-error focus-visible:ring-status-error/40"
      : "border-outline";
    return (
      <textarea
        ref={ref}
        className={`${BASE} ${DENSITY[density]} ${borderColor} ${className}`}
        aria-invalid={invalid || undefined}
        {...rest}
      />
    );
  },
);
