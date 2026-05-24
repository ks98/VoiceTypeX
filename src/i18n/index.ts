// SPDX-License-Identifier: GPL-3.0-or-later
//
// React-Bindings fuer das i18n-System.
//
// - `useT()` liefert die Translation-Function, gebunden an die aktuelle
//   Locale aus dem Zustand-Store. Re-rendert Consumer beim Locale-Wechsel.
// - `useLocale()` liest die aktuelle Locale.
// - `useI18nStore` ist der Zustand-Store — direkt fuer non-React-Code
//   (z.B. Format-Helpers) oder fuer den Bootstrap-Setter in main.tsx.
//
// Dictionaries werden eager geladen (5 Locales × ~5 KB → vernachlaessigbar).
// Lazy-Loading bringt erst bei vielen Sprachen mit grossen Translations
// einen Vorteil und macht den Bootstrap-Pfad komplizierter.
//
// **Bootstrap-Hinweis (Phase-6-Fallstrick):** `useI18nStore.setLocale()`
// aendert nur den lokalen Webview-State. Beim User-sichtbaren Locale-
// Switcher in Phase 6 muss zusaetzlich (a) `ipcSetSettings` mit dem
// neuen Wert laufen und (b) ein Tauri-Event an die anderen Webviews
// (overlay, menu) emittiert werden, die ihren Store ebenfalls
// aktualisieren — sonst bleiben sie auf der Bootstrap-Locale bis zum
// Neustart, weil Zustand-Stores nicht cross-Window geteilt sind.

import { useCallback } from "react";
import { create } from "zustand";
import {
  translate,
  type Dictionary,
  type TranslateParams,
} from "./translate";
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

// JSON-Imports werden von TS mit den konkreten Key-Typen aus der Datei
// inferiert. Wir widen explizit auf `Dictionary` — die echte
// Key-Konsistenz prueft `scripts/i18n-check.mjs` (Build-Gate).
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
  // DEFAULT_LOCALE als Initial-Wert — der echte Wert wird im Bootstrap
  // (main.tsx) aus Settings gesetzt, bevor App rendert.
  locale: DEFAULT_LOCALE,
  setLocale: (locale) => set({ locale }),
}));

export type TranslateFn = (
  key: string,
  params?: TranslateParams,
) => string;

/**
 * React-Hook fuer Translations. Re-rendert die Consumer-Component, wenn
 * die Locale wechselt; die zurueckgegebene Function ist via `useCallback`
 * stabil pro Locale.
 *
 * Konvention: NICHT als Prop weiterreichen. Stattdessen in jeder
 * uebersetzenden Component erneut `useT()` aufrufen — der Hook ist
 * trivial teuer, und so bleibt die Locale-Abhaengigkeit explizit. Sonst
 * wuerde z.B. ein `React.memo`-Wrapper auf der Consumer-Component
 * Locale-Wechsel verschlucken, weil `t` als Prop referenz-stabil bleibt.
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
 * Non-React-Zugriff auf die aktuelle Locale. Fuer Module wie
 * `format.ts`, die ausserhalb von Hooks formatieren, oder fuer
 * Test-/Debug-Pfade.
 */
export function getCurrentLocale(): SupportedLocale {
  return useI18nStore.getState().locale;
}

export { SUPPORTED_LOCALES, DEFAULT_LOCALE };
export type { SupportedLocale };
export { pickSupported } from "./detect";
