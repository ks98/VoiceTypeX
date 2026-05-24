#!/usr/bin/env node
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Build-Gate fuer das i18n-System.
//
// Prueft drei Eigenschaften:
//
//   1. **Locale-Parity**: alle Locales haben dieselben Keys wie en.json
//      (Source-of-Truth). Fehlende oder ueberzaehlige Keys werden
//      gemeldet.
//
//   2. **Used-but-undefined**: jeder `t("...")`-Aufruf im Source-Code
//      referenziert einen Key, der in en.json existiert. Plural-Base-
//      Keys (z.B. `t("logs.missed", { count: N })`) gelten als
//      bekannt, wenn `<key>.one`/`.other`/etc. in en.json existiert.
//
//   3. **Template-Literal-Prefix-Check**: `t(\`app.tabs.${id}\`)`-Calls
//      werden auf den statischen Praefix reduziert; es muss mindestens
//      ein Key in en.json mit diesem Praefix existieren — sonst ist
//      der Praefix vermutlich vertippt.
//
// Exit-Codes:
//   0: alles ok (Warnings erlaubt)
//   1: User-Fehler (fehlende Keys, used-but-undefined, broken prefix)
//   2: Tool-Fehler (JSON-Parse, fehlende Datei, FS-Crash)

import { readFile, readdir } from "node:fs/promises";
import { resolve, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const PROJECT_ROOT = resolve(fileURLToPath(import.meta.url), "..", "..");
const LOCALES_DIR = join(PROJECT_ROOT, "src", "i18n", "locales");
const SOURCE_OF_TRUTH = "en.json";
const SRC_DIR = join(PROJECT_ROOT, "src");
const EXCLUDE_DIRS = new Set(["node_modules", "i18n"]);
const SOURCE_EXTS = new Set([".ts", ".tsx"]);

async function loadLocale(name) {
  const path = join(LOCALES_DIR, name);
  const raw = await readFile(path, "utf8");
  const parsed = JSON.parse(raw);
  return { name, keys: new Set(Object.keys(parsed)) };
}

async function findSourceFiles(dir) {
  const out = [];
  const entries = await readdir(dir, { withFileTypes: true });
  for (const entry of entries) {
    if (entry.name.startsWith(".")) continue;
    if (EXCLUDE_DIRS.has(entry.name)) continue;
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...(await findSourceFiles(full)));
    } else if (entry.isFile()) {
      const dot = entry.name.lastIndexOf(".");
      if (dot >= 0 && SOURCE_EXTS.has(entry.name.slice(dot))) {
        out.push(full);
      }
    }
  }
  return out;
}

// Match t("..."), t('...'), t(`...`). Capture the literal content.
// Lookbehind statt `\b`, weil `\b` zwischen `$`/`_` und `t` triggert
// (was bei TS-Identifiern wie `$t(` falsche Treffer waere). Non-greedy,
// kein Escape-Handling — Translation-Keys haben keine Quotes drin.
const T_CALL_RE = /(?<![A-Za-z0-9_$])t\(\s*(["'`])([^"'`]*?)\1/g;

// CLDR-Plural-Kategorien. Wird benutzt, um zu pruegfen, ob ein
// referenzierter Base-Key (`t("logs.missed", {count:5})`) via einer
// dieser Suffix-Varianten in en.json verfuegbar ist.
const PLURAL_FORMS = ["zero", "one", "two", "few", "many", "other"];

function keyOrPluralBaseExists(key, truthKeys) {
  if (truthKeys.has(key)) return true;
  for (const form of PLURAL_FORMS) {
    if (truthKeys.has(`${key}.${form}`)) return true;
  }
  return false;
}

function extractUsage(content) {
  const literal = []; // exact key
  const prefix = []; // template-literal prefix before first ${...}
  for (const m of content.matchAll(T_CALL_RE)) {
    const quote = m[1];
    const body = m[2];
    if (quote === "`" && body.includes("${")) {
      const cut = body.indexOf("${");
      const pre = body.slice(0, cut);
      if (pre.length > 0) prefix.push(pre);
    } else {
      literal.push(body);
    }
  }
  return { literal, prefix };
}

function setDiff(a, b) {
  const out = [];
  for (const k of a) if (!b.has(k)) out.push(k);
  return out.sort();
}

async function main() {
  // Source-of-truth zuerst.
  const truth = await loadLocale(SOURCE_OF_TRUTH);
  const truthKeys = truth.keys;

  // Alle anderen Locales.
  const localeFiles = (await readdir(LOCALES_DIR))
    .filter((f) => f.endsWith(".json"))
    .filter((f) => f !== SOURCE_OF_TRUTH);
  const otherLocales = await Promise.all(localeFiles.map(loadLocale));

  let errors = 0;
  let warnings = 0;

  // 1) Locale-Parity.
  for (const loc of otherLocales) {
    const missing = setDiff(truthKeys, loc.keys);
    const extra = setDiff(loc.keys, truthKeys);
    if (missing.length > 0) {
      console.error(
        `ERROR ${loc.name}: ${missing.length} missing key(s):\n  - ${missing.join("\n  - ")}`,
      );
      errors += missing.length;
    }
    if (extra.length > 0) {
      console.warn(
        `WARN  ${loc.name}: ${extra.length} extra key(s) not in ${SOURCE_OF_TRUTH}:\n  - ${extra.join("\n  - ")}`,
      );
      warnings += extra.length;
    }
  }

  // 2 + 3) Source-Scan.
  const files = await findSourceFiles(SRC_DIR);
  const literalSeen = new Set();
  const prefixSeen = new Map(); // prefix → first occurrence file
  for (const f of files) {
    const content = await readFile(f, "utf8");
    const { literal, prefix } = extractUsage(content);
    for (const k of literal) {
      literalSeen.add(k);
      if (!keyOrPluralBaseExists(k, truthKeys)) {
        console.error(
          `ERROR ${relative(PROJECT_ROOT, f)}: key "${k}" used but not in ${SOURCE_OF_TRUTH}`,
        );
        errors++;
      }
    }
    for (const p of prefix) {
      if (!prefixSeen.has(p)) prefixSeen.set(p, f);
      const anyMatch = [...truthKeys].some((k) => k.startsWith(p));
      if (!anyMatch) {
        console.error(
          `ERROR ${relative(PROJECT_ROOT, f)}: template-literal prefix "${p}*" has no matching key in ${SOURCE_OF_TRUTH}`,
        );
        errors++;
      }
    }
  }

  const allUsed = literalSeen.size + prefixSeen.size;
  console.log(
    `i18n-check: ${truthKeys.size} truth-keys, ${otherLocales.length} other locales, ${allUsed} usages scanned`,
  );
  if (errors > 0) {
    console.error(`i18n-check: FAILED with ${errors} error(s), ${warnings} warning(s)`);
    process.exit(1);
  }
  if (warnings > 0) {
    console.warn(`i18n-check: passed with ${warnings} warning(s)`);
  } else {
    console.log("i18n-check: OK");
  }
}

main().catch((e) => {
  console.error("i18n-check crashed:", e);
  process.exit(2);
});
