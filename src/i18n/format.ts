// SPDX-License-Identifier: GPL-3.0-or-later
//
// Wrapper um `Intl.NumberFormat` und `Intl.DateTimeFormat`. Werden ab
// Phase 1 fuer Groessen-Anzeigen (`5,1 GB` vs. `5.1 GB`) und Timestamps
// gebraucht. Bewusst nur Plain-Funktionen ohne React-Hook-Wrapper —
// `Intl.*`-Allokation kostet µs, ein useMemo daher nutzlos (und
// invalidiert bei jedem inline-Optionen-Literal eh sofort).
//
// Aufruf-Konvention in React-Komponenten:
//   const locale = useLocale();
//   const gb = formatNumber(value, locale, { maximumFractionDigits: 1 });

export function formatNumber(
  value: number,
  locale: string,
  options?: Intl.NumberFormatOptions,
): string {
  return new Intl.NumberFormat(locale, options).format(value);
}

export function formatDate(
  value: Date | number,
  locale: string,
  options?: Intl.DateTimeFormatOptions,
): string {
  return new Intl.DateTimeFormat(locale, options).format(value);
}
