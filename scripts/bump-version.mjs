// SPDX-License-Identifier: GPL-3.0-or-later
//
// Single-Source-Versionsbump fuer VoiceTypeX.
//
// Quelle der Wahrheit ist `src-tauri/Cargo.toml` ([package] version) — die
// App liest ihre Anzeige-Version aus `CARGO_PKG_VERSION`, und
// `tauri.conf.json` hat KEIN `version`-Feld mehr (Tauri erbt es dann aus
// Cargo.toml). Dieses Script haelt die zwei verbleibenden Stellen synchron,
// die Cargo nicht selbst erbt:
//   1. src-tauri/Cargo.toml  ([package] version)
//   2. package.json          (version)
// und aktualisiert zusaetzlich den voicetypex-Eintrag in
// src-tauri/Cargo.lock, damit der Release-Commit in sich konsistent ist.
//
// Aufruf (laeuft via pnpm im Repo-Root, daher relative Pfade):
//   pnpm release patch|minor|major      # bumpt vom aktuellen Stand
//   pnpm release 1.2.3                   # explizite Version (auch -rc.1)
//   pnpm release patch --dry-run         # nur anzeigen, nichts schreiben
//   pnpm release patch --no-git          # Dateien bumpen, ohne commit+tag
//
// Default (ohne --no-git): committet die drei Dateien als
// `chore(release): vX.Y.Z` und setzt das annotierte Tag `vX.Y.Z`. Der Push
// (→ CI-Release-Pipeline) bleibt ein bewusster, manueller Schritt.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";

const CARGO_TOML = "src-tauri/Cargo.toml";
const CARGO_LOCK = "src-tauri/Cargo.lock";
const PACKAGE_JSON = "package.json";

// X.Y.Z mit optionalem Pre-Release-Suffix (z.B. 1.0.0-rc.1).
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function die(msg) {
  console.error(`[bump-version] ${msg}`);
  process.exit(1);
}

// --- Argumente -------------------------------------------------------------
const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const noGit = args.includes("--no-git");
const positional = args.filter((a) => !a.startsWith("--"));
if (positional.length !== 1) {
  die("genau ein Argument erwartet: patch | minor | major | <x.y.z>");
}
const spec = positional[0];

// --- Aktuelle Version aus Cargo.toml [package] lesen ----------------------
const cargoText = readFileSync(CARGO_TOML, "utf8");
const current = readPackageVersion(cargoText);
if (!current) die("konnte [package] version in Cargo.toml nicht finden");

// --- Neue Version bestimmen ------------------------------------------------
let next;
if (SEMVER.test(spec)) {
  next = spec;
} else if (spec === "major" || spec === "minor" || spec === "patch") {
  next = bump(current, spec);
} else {
  die(`ungueltiges Argument '${spec}': patch | minor | major | <x.y.z>`);
}
if (next === current) die(`neue Version identisch mit aktueller (${current})`);

console.log(`[bump-version] ${current} -> ${next}`);

// --- Transformationen vorbereiten (noch nicht schreiben) ------------------
const newCargo = replacePackageVersion(cargoText, next);

const pkgText = readFileSync(PACKAGE_JSON, "utf8");
const pkgHits = pkgText.match(/"version":\s*"[^"]+"/g) || [];
if (pkgHits.length !== 1) {
  die(`erwartete genau ein "version"-Feld in package.json, fand ${pkgHits.length}`);
}
const newPkg = pkgText.replace(/("version":\s*")[^"]+(")/, `$1${next}$2`);

let newLock = null;
if (existsSync(CARGO_LOCK)) {
  const lockText = readFileSync(CARGO_LOCK, "utf8");
  const lockRe = /(name = "voicetypex"\nversion = ")[^"]+(")/;
  if (!lockRe.test(lockText)) {
    die("konnte voicetypex-Eintrag in Cargo.lock nicht finden");
  }
  newLock = lockText.replace(lockRe, `$1${next}$2`);
}

// --- Dry-Run: nur berichten -----------------------------------------------
if (dryRun) {
  console.log("[bump-version] --dry-run: keine Dateien geschrieben.");
  console.log(`  ${CARGO_TOML}  [package] version -> ${next}`);
  console.log(`  ${PACKAGE_JSON}          version -> ${next}`);
  if (newLock) console.log(`  ${CARGO_LOCK}  voicetypex        -> ${next}`);
  process.exit(0);
}

// --- Arbeitsbaum pruefen (vor dem Schreiben) ------------------------------
if (!noGit) {
  if (git(["status", "--porcelain"]).trim()) {
    die("Arbeitsbaum nicht sauber — committe/stashe erst, dann release.");
  }
  if (git(["tag", "--list", `v${next}`]).trim()) {
    die(`Tag v${next} existiert bereits.`);
  }
}

// --- Schreiben -------------------------------------------------------------
writeFileSync(CARGO_TOML, newCargo);
writeFileSync(PACKAGE_JSON, newPkg);
if (newLock) writeFileSync(CARGO_LOCK, newLock);

if (noGit) {
  console.log("[bump-version] Dateien gebumpt (--no-git: kein commit/tag).");
  process.exit(0);
}

// --- Commit + annotiertes Tag ---------------------------------------------
const files = [CARGO_TOML, PACKAGE_JSON];
if (newLock) files.push(CARGO_LOCK);
git(["add", ...files]);
git(["commit", "-m", `chore(release): v${next}`]);
git(["tag", "-a", `v${next}`, "-m", `v${next}`]);
console.log(`[bump-version] committet + getaggt: v${next}`);
console.log("[bump-version] Push mit:  git push --follow-tags");

// --- Helper ----------------------------------------------------------------

// Liest die Version aus der [package]-Section — NICHT aus [dependencies]
// o.ae., wo Dutzende `version = "…"`-Zeilen stehen.
function readPackageVersion(text) {
  let inPackage = false;
  for (const line of text.split("\n")) {
    if (/^\[package\]\s*$/.test(line)) {
      inPackage = true;
      continue;
    }
    if (inPackage && /^\[/.test(line)) break; // naechste Section erreicht
    if (inPackage) {
      const m = line.match(/^version\s*=\s*"([^"]+)"/);
      if (m) return m[1];
    }
  }
  return null;
}

function replacePackageVersion(text, version) {
  const lines = text.split("\n");
  let inPackage = false;
  for (let i = 0; i < lines.length; i++) {
    if (/^\[package\]\s*$/.test(lines[i])) {
      inPackage = true;
      continue;
    }
    if (inPackage && /^\[/.test(lines[i])) break;
    if (inPackage && /^version\s*=\s*"[^"]+"/.test(lines[i])) {
      lines[i] = lines[i].replace(/^(version\s*=\s*")[^"]+(")/, `$1${version}$2`);
      return lines.join("\n");
    }
  }
  die("konnte [package] version in Cargo.toml nicht ersetzen");
  return text; // unerreichbar (die() beendet), beruhigt aber den Linter
}

// Keyword-Bump: arbeitet auf dem numerischen Kern, verwirft ein evtl.
// Pre-Release-Suffix. Zum Finalisieren eines -rc die Version explizit angeben.
function bump(version, kind) {
  const core = version.split("-")[0];
  const [maj, min, pat] = core.split(".").map((n) => parseInt(n, 10));
  if ([maj, min, pat].some((n) => Number.isNaN(n))) {
    die(`unparsebare Version: ${version}`);
  }
  if (kind === "major") return `${maj + 1}.0.0`;
  if (kind === "minor") return `${maj}.${min + 1}.0`;
  return `${maj}.${min}.${pat + 1}`;
}

function git(argv) {
  try {
    return execFileSync("git", argv, { encoding: "utf8" });
  } catch (e) {
    die(`git ${argv.join(" ")} fehlgeschlagen: ${e.message}`);
  }
}
