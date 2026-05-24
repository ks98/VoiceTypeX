// SPDX-License-Identifier: GPL-3.0-or-later
//
// Supported-Locale-Set und OS-Locale-Mapping.
//
// Die eigentliche OS-Locale-Detection laeuft im Backend (siehe
// src-tauri/src/lib.rs) und wird in `Settings.locale` persistiert —
// dadurch kein Race zwischen den drei Webview-Fenstern (main, overlay,
// menu), die alle ueber dieselbe Settings-Datei laufen. Diese Datei
// haelt nur den Frontend-seitigen Mapper bereit, damit z.B. ein
// spaeterer User-Switcher "Auto/System" das gleiche Mapping nutzen
// kann.

export const SUPPORTED_LOCALES = ["en", "de", "fr", "es", "it"] as const;

export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export const DEFAULT_LOCALE: SupportedLocale = "en";

/**
 * Mappt einen rohen Locale-String (BCP-47 wie "de-DE", "en_US",
 * "pt-BR" oder schlicht "de") auf eine der unterstuetzten Locales.
 * Region-Suffixe werden ignoriert. Unbekannte Sprachen fallen auf
 * DEFAULT_LOCALE.
 */
export function pickSupported(
  raw: string | null | undefined,
): SupportedLocale {
  if (!raw) return DEFAULT_LOCALE;
  const prefix = raw.split(/[-_]/)[0]?.toLowerCase();
  if (!prefix) return DEFAULT_LOCALE;
  return (SUPPORTED_LOCALES as readonly string[]).includes(prefix)
    ? (prefix as SupportedLocale)
    : DEFAULT_LOCALE;
}
