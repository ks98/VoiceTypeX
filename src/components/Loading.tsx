// SPDX-License-Identifier: GPL-3.0-or-later
import { useT } from "../i18n";

interface LoadingProps {
  /** Visible text; otherwise the default "Loading…" via i18n. */
  label?: string;
  /** Inline variant (no padding, for buttons / status lines). */
  inline?: boolean;
}

/**
 * Loading indicator — centralizes the `<div>Lade …</div>` pattern
 * that was duplicated with subtle drifts in Modes, Settings,
 * ApiKeysSection.
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
