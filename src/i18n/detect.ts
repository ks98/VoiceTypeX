// SPDX-License-Identifier: GPL-3.0-or-later
//
// Supported-locale set and OS-locale mapping.
//
// The actual OS-locale detection runs in the backend (see
// src-tauri/src/lib.rs) and is persisted in `Settings.locale` —
// avoiding a race between the three webview windows (main, overlay,
// menu) that all share the same settings file. This file only holds
// the frontend-side mapper, so e.g. a later user "Auto/System"
// switcher can use the same mapping.

export const SUPPORTED_LOCALES = ["en", "de", "fr", "es", "it"] as const;

export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export const DEFAULT_LOCALE: SupportedLocale = "en";

/**
 * Maps a raw locale string (BCP-47 like "de-DE", "en_US", "pt-BR" or
 * just "de") to one of the supported locales. Region suffixes are
 * ignored. Unknown languages fall back to `DEFAULT_LOCALE`.
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

/**
 * Native language name per locale for the user switcher in settings.
 * Deliberately not resolved via `t()`, because the switcher shows
 * languages the user has _not yet_ activated — instead every language
 * appears in its own form (classic locale-picker UX).
 */
export const LOCALE_NATIVE_NAMES: Record<SupportedLocale, string> = {
  en: "English",
  de: "Deutsch",
  fr: "Français",
  es: "Español",
  it: "Italiano",
};
