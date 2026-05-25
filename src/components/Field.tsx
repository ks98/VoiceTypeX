// SPDX-License-Identifier: GPL-3.0-or-later
import type { ReactNode } from "react";

interface FieldProps {
  label: string;
  hint?: string;
  /**
   * Optional ID. When set:
   * - `<label htmlFor={id}>` ties the label to the (first) input
   *   element with this `id` attribute.
   * - The hint gets `id={`${id}-hint`}`; inputs can set
   *   `aria-describedby` to that.
   *
   * If not set, the label falls back to a `<label>` element without
   * binding — still functional, but weaker a11y.
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
