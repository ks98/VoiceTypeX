// SPDX-License-Identifier: GPL-3.0-or-later
//
// React bindings for the i18n system.
//
// - `useT()` returns the translation function, bound to the current
//   locale from the Zustand store. Re-renders consumers on locale change.
// - `useLocale()` reads the current locale.
// - `useI18nStore` is the Zustand store — used directly from non-React
//   code (e.g. format helpers) or for the bootstrap setter in main.tsx.
//
// Dictionaries are loaded eagerly (5 locales × ~5 KB → negligible).
// Lazy-loading only pays off with many languages and large translation
// files, and it complicates the bootstrap path.
//
// **Bootstrap note (phase-6 pitfall):** `useI18nStore.setLocale()` only
// changes the local webview state. For the user-facing locale switcher
// in phase 6 we additionally need to (a) call `ipcSetSettings` with the
// new value and (b) emit a Tauri event to the other webviews (overlay,
// menu) so their stores update as well — otherwise they stay on the
// bootstrap locale until restart, because Zustand stores are not shared
// across windows.

import { useCallback } from "react";
import { create } from "zustand";
import { translate, type Dictionary, type TranslateParams } from "./translate";
import {
  DEFAULT_LOCALE,
  SUPPORTED_LOCALES,
  type SupportedLocale,
} from "./detect";

import en from "./locales/en.json";
import de from "./locales/de.json";
import fr from "./locales/fr.json";
import es from "./locales/es.json";
import it from "./locales/it.json";

// TS infers JSON imports with the concrete key types from the file. We
// explicitly widen to `Dictionary` — actual key consistency is
// verified by `scripts/i18n-check.mjs` (build gate).
const DICTIONARIES: Record<SupportedLocale, Dictionary> = {
  en: en as Dictionary,
  de: de as Dictionary,
  fr: fr as Dictionary,
  es: es as Dictionary,
  it: it as Dictionary,
};

const FALLBACK_DICT: Dictionary = DICTIONARIES[DEFAULT_LOCALE];

interface I18nState {
  locale: SupportedLocale;
  setLocale: (locale: SupportedLocale) => void;
}

export const useI18nStore = create<I18nState>((set) => ({
  // DEFAULT_LOCALE as initial value — the real value is set during
  // bootstrap (main.tsx) from settings before the app renders.
  locale: DEFAULT_LOCALE,
  setLocale: (locale) => set({ locale }),
}));

export type TranslateFn = (key: string, params?: TranslateParams) => string;

/**
 * React hook for translations. Re-renders the consuming component when
 * the locale changes; the returned function is stable per locale via
 * `useCallback`.
 *
 * Convention: do NOT pass it down as a prop. Instead, call `useT()`
 * again in every translating component — the hook is trivially cheap
 * and this keeps the locale dependency explicit. Otherwise a
 * `React.memo` wrapper on the consumer would swallow locale changes,
 * because `t` would stay reference-stable as a prop.
 */
export function useT(): TranslateFn {
  const locale = useI18nStore((s) => s.locale);
  return useCallback(
    (key, params) =>
      translate(key, params, {
        locale,
        current: DICTIONARIES[locale],
        fallback: FALLBACK_DICT,
      }),
    [locale],
  );
}

export function useLocale(): SupportedLocale {
  return useI18nStore((s) => s.locale);
}

/**
 * Non-React access to the current locale. For modules like `format.ts`
 * that format outside of hooks, or for test/debug paths.
 */
export function getCurrentLocale(): SupportedLocale {
  return useI18nStore.getState().locale;
}

export { SUPPORTED_LOCALES, DEFAULT_LOCALE };
export type { SupportedLocale };
export { pickSupported, LOCALE_NATIVE_NAMES } from "./detect";
