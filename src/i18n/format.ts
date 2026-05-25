// SPDX-License-Identifier: GPL-3.0-or-later
//
// Wrappers around `Intl.NumberFormat` and `Intl.DateTimeFormat`. Used
// from phase 1 onward for size displays (`5,1 GB` vs. `5.1 GB`) and
// timestamps. Deliberately plain functions without a React-hook
// wrapper — `Intl.*` allocation costs microseconds, so `useMemo` is
// useless (and would invalidate on every inline options literal
// anyway).
//
// Calling convention in React components:
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
