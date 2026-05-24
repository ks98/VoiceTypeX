// SPDX-License-Identifier: GPL-3.0-or-later
import { useT } from "../i18n";

interface LoadingProps {
  /** Sichtbarer Text, sonst Default "Loading…" via i18n. */
  label?: string;
  /** Inline-Variante (kein Padding, fuer Buttons / Status-Zeilen). */
  inline?: boolean;
}

/**
 * Loading-Indikator — zentralisiert das `<div>Lade …</div>`-Muster, das
 * in Modes, Settings, ApiKeysSection mit subtilen Drifts dupliziert war.
 */
export default function Loading({ label, inline }: LoadingProps): JSX.Element {
  const t = useT();
  const text = label ?? t("common.loading");
  if (inline) {
    return (
      <span className="text-sm text-fg-faint" role="status">
        {text}
      </span>
    );
  }
  return (
    <div className="text-sm text-fg-faint py-2" role="status">
      {text}
    </div>
  );
}
