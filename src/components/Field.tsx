// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

interface FieldProps {
  label: string;
  hint?: string;
  /**
   * Optionale ID. Wenn gesetzt:
   * - `<label htmlFor={id}>` bindet das Label an das (erste) Input-Element
   *   mit diesem `id`-Attribut.
   * - Der Hint bekommt `id={`${id}-hint`}`; Inputs koennen `aria-describedby`
   *   darauf setzen.
   *
   * Wenn nicht gesetzt, faellt das Label auf ein `<label>`-Element ohne
   * Bindung zurueck — weiter funktionsfähig, aber schwächere A11y.
   */
  htmlFor?: string;
  children: ReactNode;
}

export default function Field({
  label,
  hint,
  htmlFor,
  children,
}: FieldProps): JSX.Element {
  const hintId = htmlFor ? `${htmlFor}-hint` : undefined;
  return (
    <div className="flex flex-col gap-1.5">
      <label
        className="text-sm font-medium text-fg-muted"
        htmlFor={htmlFor}
      >
        {label}
      </label>
      {children}
      {hint ? (
        <p id={hintId} className="text-xs text-fg-faint">
          {hint}
        </p>
      ) : null}
    </div>
  );
}
