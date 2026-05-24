// SPDX-License-Identifier: GPL-3.0-or-later
//
// Reine Translation-Logik — keine React-Deps, keine Side-Effects.
// Wird vom `useT()`-Hook (siehe `./index.ts`) konsumiert und ist isoliert
// testbar.
//
// Konventionen:
// - Keys sind flache Strings mit Punkten als Separator (`"app.title"`).
//   Flach (nicht nested) wegen einfacher Diff-Reviews und trivialer
//   Key-Listung im `scripts/i18n-check.mjs`.
// - Interpolation: `{name}` im Template, `params: { name: "X" }`.
// - Plural: wenn `params.count` eine Zahl ist, wird das Key-Suffix
//   `.one`/`.other`/... ueber `Intl.PluralRules` ausgewaehlt.
// - Fallback-Kette: current-locale → fallback (en) → key selbst.
//   Letzteres ist Dev-Visibility, damit fehlende Keys nicht still
//   leere Strings zurueckgeben.

export type Dictionary = Readonly<Record<string, string>>;

export interface TranslateParams {
  readonly [key: string]: string | number;
}

export interface TranslateContext {
  /** BCP-47-Locale-Code, z.B. "de" oder "fr". Steuert die Plural-Form. */
  readonly locale: string;
  /** Dictionary der aktuellen Locale. */
  readonly current: Dictionary;
  /** Fallback-Dictionary (Source-of-Truth, normalerweise `en`). */
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
 * Lookup eines Translation-Keys gegen current + fallback Dictionary.
 *
 * - Interpoliert `{name}`-Platzhalter aus `params`.
 * - Bei numerischem `params.count`: zuerst pluralisierter Key
 *   (`<key>.<form>`), dann `<key>.other`, dann `<key>` ohne Suffix.
 * - Fallback bei Key-Miss: erst `fallback`-Dict, dann der Key selbst
 *   (sichtbar im UI, signalisiert Dev-Bug).
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
