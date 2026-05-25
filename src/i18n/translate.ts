// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pure translation logic — no React deps, no side effects.
// Consumed by the `useT()` hook (see `./index.ts`) and tested in
// isolation.
//
// Conventions:
// - Keys are flat strings with dots as separators (`"app.title"`).
//   Flat (not nested) for easy diff reviews and trivial key listing
//   in `scripts/i18n-check.mjs`.
// - Interpolation: `{name}` in the template, `params: { name: "X" }`.
// - Plural: if `params.count` is a number, the key suffix
//   `.one`/`.other`/... is selected via `Intl.PluralRules`.
// - Fallback chain: current locale → fallback (en) → the key itself.
//   The latter is dev visibility so missing keys don't silently
//   return empty strings.

export type Dictionary = Readonly<Record<string, string>>;

export interface TranslateParams {
  readonly [key: string]: string | number;
}

export interface TranslateContext {
  /** BCP-47 locale code, e.g. "de" or "fr". Controls the plural
   * form. */
  readonly locale: string;
  /** Dictionary of the current locale. */
  readonly current: Dictionary;
  /** Fallback dictionary (source-of-truth, usually `en`). */
  readonly fallback: Dictionary;
}

const PARAM_RE = /\{(\w+)\}/g;

function interpolate(
  template: string,
  params: TranslateParams | undefined,
): string {
  if (!params) return template;
  return template.replace(PARAM_RE, (full, name: string) => {
    const value = params[name];
    return value === undefined ? full : String(value);
  });
}

function selectPluralSuffix(count: number, locale: string): Intl.LDMLPluralRule {
  return new Intl.PluralRules(locale).select(count);
}

/**
 * Look up a translation key against the current + fallback dictionary.
 *
 * - Interpolates `{name}` placeholders from `params`.
 * - For numeric `params.count`: first the pluralized key
 *   (`<key>.<form>`), then `<key>.other`, then `<key>` without
 *   suffix.
 * - Fallback on key miss: first the `fallback` dict, then the key
 *   itself (visible in the UI, signals a dev bug).
 */
export function translate(
  key: string,
  params: TranslateParams | undefined,
  ctx: TranslateContext,
): string {
  const count = params?.count;
  const candidates: string[] = [];
  if (typeof count === "number") {
    candidates.push(`${key}.${selectPluralSuffix(count, ctx.locale)}`);
    candidates.push(`${key}.other`);
  }
  candidates.push(key);

  for (const candidate of candidates) {
    const template = ctx.current[candidate] ?? ctx.fallback[candidate];
    if (template !== undefined) {
      return interpolate(template, params);
    }
  }
  return key;
}
