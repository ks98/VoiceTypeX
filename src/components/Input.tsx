// SPDX-License-Identifier: GPL-3.0-or-later
import { forwardRef } from "react";
import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";

type Density = "default" | "compact";

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  /** "default" (px-3 py-2, fuer Settings/Wizard) | "compact" (px-2 py-1.5, fuer Modal-Editor). */
  density?: Density;
  /** Zusaetzliche Visuelle Markierung fuer Validierungs-Fehler. */
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
 * Konsolidiertes Input-Element. Ersetzt das 3× duplizierte `inputCls`-
 * Pattern aus Settings.tsx, OnboardingWizard.tsx und ModeEditor.tsx
 * (mit subtil unterschiedlichen Paddings).
 *
 * Density:
 * - `default` — fuer Settings, Onboarding, normale Formulare.
 * - `compact` — fuer Modal-Editor mit hoher Feldzahl.
 *
 * `invalid` rendert eine rote Border + roten Focus-Ring; ergaenze fuer
 * volle A11y zusaetzlich `aria-invalid` und einen sichtbaren Hilfetext
 * (z.B. via Field-hint).
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
