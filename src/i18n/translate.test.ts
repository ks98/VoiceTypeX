// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { translate, type Dictionary } from "./translate";

const en: Dictionary = {
  "app.title": "VoiceTypeX",
  "modes.confirm_delete": 'Really delete mode "{name}"?',
  "logs.missed.one": "{count} new entry while paused",
  "logs.missed.other": "{count} new entries while paused",
  "ratio": "{ratio}",
};

const de: Dictionary = {
  "app.title": "VoiceTypeX",
  "modes.confirm_delete": "Modus „{name}\" wirklich löschen?",
  "logs.missed.one": "{count} neuer Eintrag während Pause",
  "logs.missed.other": "{count} neue Einträge während Pause",
};

const ctxDe = { locale: "de", current: de, fallback: en };
const ctxEn = { locale: "en", current: en, fallback: en };

describe("translate", () => {
  it("returns translation from current locale", () => {
    expect(translate("app.title", undefined, ctxDe)).toBe("VoiceTypeX");
  });

  it("interpolates {name}", () => {
    expect(translate("modes.confirm_delete", { name: "Exakt" }, ctxDe)).toBe(
      'Modus „Exakt" wirklich löschen?',
    );
  });

  it("interpolates numbers as strings", () => {
    expect(translate("ratio", { ratio: 0.42 }, ctxEn)).toBe("0.42");
  });

  it("leaves placeholder intact when param missing", () => {
    expect(translate("modes.confirm_delete", {}, ctxDe)).toBe(
      "Modus „{name}\" wirklich löschen?",
    );
  });

  it("picks plural .one for count=1", () => {
    expect(translate("logs.missed", { count: 1 }, ctxEn)).toBe(
      "1 new entry while paused",
    );
  });

  it("picks plural .other for count=5", () => {
    expect(translate("logs.missed", { count: 5 }, ctxEn)).toBe(
      "5 new entries while paused",
    );
  });

  it("picks plural .other for count=0 in en", () => {
    // EN behandelt 0 als "other" (CLDR-Konvention).
    expect(translate("logs.missed", { count: 0 }, ctxEn)).toBe(
      "0 new entries while paused",
    );
  });

  it("uses plural in de when current locale has it", () => {
    expect(translate("logs.missed", { count: 1 }, ctxDe)).toBe(
      "1 neuer Eintrag während Pause",
    );
    expect(translate("logs.missed", { count: 3 }, ctxDe)).toBe(
      "3 neue Einträge während Pause",
    );
  });

  it("falls back to en when current lacks key", () => {
    const partialDe: Dictionary = { "app.title": "VoiceTypeX" };
    expect(
      translate(
        "modes.confirm_delete",
        { name: "X" },
        { locale: "de", current: partialDe, fallback: en },
      ),
    ).toBe('Really delete mode "X"?');
  });

  it("falls back to en plural form when current locale missing plural", () => {
    const partialDe: Dictionary = {};
    expect(
      translate(
        "logs.missed",
        { count: 5 },
        { locale: "de", current: partialDe, fallback: en },
      ),
    ).toBe("5 new entries while paused");
  });

  it("returns key when missing in both", () => {
    expect(translate("nope.missing", undefined, ctxDe)).toBe("nope.missing");
  });

  it("returns key with plural-suffix in candidate order but key fallback", () => {
    // Wenn weder pluralisiert noch base existiert → key selbst.
    expect(translate("nope.missing", { count: 3 }, ctxDe)).toBe("nope.missing");
  });

  it("ignores count when not a number", () => {
    // count="three" → kein Plural-Pfad, direkter Key-Lookup.
    const dict: Dictionary = { plain: "Plain {count}" };
    expect(
      translate(
        "plain",
        { count: "three" },
        { locale: "en", current: dict, fallback: dict },
      ),
    ).toBe("Plain three");
  });

  it("treats NaN count as 'other'", () => {
    // Intl.PluralRules.select(NaN) liefert "other" — der Plural-Pfad
    // wird genommen, "NaN" wird als Zahl-zu-String interpoliert.
    expect(translate("logs.missed", { count: NaN }, ctxEn)).toBe(
      "NaN new entries while paused",
    );
  });

  it("replaces the same placeholder multiple times", () => {
    const dict: Dictionary = { g: "Hello {name}, welcome {name}" };
    expect(
      translate(
        "g",
        { name: "X" },
        { locale: "en", current: dict, fallback: dict },
      ),
    ).toBe("Hello X, welcome X");
  });

  it("leaves malformed placeholder intact (no closing brace)", () => {
    // `{name` ohne `}` matcht die Regex nicht → bleibt literal stehen.
    // `{greet}` weiter hinten wird normal ersetzt. Dokumentiert das
    // Best-Effort-Verhalten gegen kaputte Templates.
    const dict: Dictionary = { g: "Hi {name and {greet}!" };
    expect(
      translate(
        "g",
        { name: "X", greet: "yo" },
        { locale: "en", current: dict, fallback: dict },
      ),
    ).toBe("Hi {name and yo!");
  });
});
