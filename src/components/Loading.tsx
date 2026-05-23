// SPDX-License-Identifier: GPL-3.0-or-later

interface LoadingProps {
  /** Sichtbarer Text, sonst nur ein dezenter Indikator. */
  label?: string;
  /** Inline-Variante (kein Padding, fuer Buttons / Status-Zeilen). */
  inline?: boolean;
}

/**
 * Loading-Indikator — zentralisiert das `<div className="text-fg-faint">Lade …</div>`-
 * Muster, das in Modes, Settings, ApiKeysSection mit subtilen
 * Padding/Text-Size-Drift dupliziert war.
 */
export default function Loading({ label, inline }: LoadingProps): JSX.Element {
  if (inline) {
    return (
      <span className="text-sm text-fg-faint" role="status">
        {label ?? "Lade …"}
      </span>
    );
  }
  return (
    <div className="text-sm text-fg-faint py-2" role="status">
      {label ?? "Lade …"}
    </div>
  );
}
