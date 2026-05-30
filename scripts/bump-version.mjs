// SPDX-License-Identifier: GPL-3.0-or-later
//
// Single-source version bump for VoiceTypeX.
//
// The source of truth is `src-tauri/Cargo.toml` ([package] version) — the
// app reads its display version from `CARGO_PKG_VERSION`, and
// `tauri.conf.json` no longer has a `version` field (Tauri then inherits it
// from Cargo.toml). This script keeps the two remaining spots in sync that
// Cargo does not inherit on its own:
//   1. src-tauri/Cargo.toml  ([package] version)
//   2. package.json          (version)
// and additionally updates the voicetypex entry in
// src-tauri/Cargo.lock, so the release commit is internally consistent.
//
// Invocation (runs via pnpm in the repo root, hence relative paths):
//   pnpm release patch|minor|major      # bumps from the current state
//   pnpm release 1.2.3                   # explicit version (also -rc.1)
//   pnpm release patch --dry-run         # only show, write nothing
//   pnpm release patch --no-git          # bump files, without commit+tag
//
// Default (without --no-git): commits the three files as
// `chore(release): vX.Y.Z` and sets the annotated tag `vX.Y.Z`. The push
// (→ CI release pipeline) stays a deliberate, manual step.

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";

const CARGO_TOML = "src-tauri/Cargo.toml";
const CARGO_LOCK = "src-tauri/Cargo.lock";
const PACKAGE_JSON = "package.json";

// X.Y.Z with an optional pre-release suffix (e.g. 1.0.0-rc.1).
const SEMVER = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;

function die(msg) {
  console.error(`[bump-version] ${msg}`);
  process.exit(1);
}

// --- Arguments -------------------------------------------------------------
const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const noGit = args.includes("--no-git");
const positional = args.filter((a) => !a.startsWith("--"));
if (positional.length !== 1) {
  die("expected exactly one argument: patch | minor | major | <x.y.z>");
}
const spec = positional[0];

// --- Read the current version from Cargo.toml [package] -------------------
const cargoText = readFileSync(CARGO_TOML, "utf8");
const current = readPackageVersion(cargoText);
if (!current) die("could not find [package] version in Cargo.toml");

// --- Determine the new version ---------------------------------------------
let next;
if (SEMVER.test(spec)) {
  next = spec;
} else if (spec === "major" || spec === "minor" || spec === "patch") {
  next = bump(current, spec);
} else {
  die(`invalid argument '${spec}': patch | minor | major | <x.y.z>`);
}
if (next === current) die(`new version identical to current (${current})`);

console.log(`[bump-version] ${current} -> ${next}`);

// --- Prepare the transformations (do not write yet) -----------------------
const newCargo = replacePackageVersion(cargoText, next);

const pkgText = readFileSync(PACKAGE_JSON, "utf8");
const pkgHits = pkgText.match(/"version":\s*"[^"]+"/g) || [];
if (pkgHits.length !== 1) {
  die(`expected exactly one "version" field in package.json, found ${pkgHits.length}`);
}
const newPkg = pkgText.replace(/("version":\s*")[^"]+(")/, `$1${next}$2`);

let newLock = null;
if (existsSync(CARGO_LOCK)) {
  const lockText = readFileSync(CARGO_LOCK, "utf8");
  const lockRe = /(name = "voicetypex"\nversion = ")[^"]+(")/;
  if (!lockRe.test(lockText)) {
    die("could not find the voicetypex entry in Cargo.lock");
  }
  newLock = lockText.replace(lockRe, `$1${next}$2`);
}

// --- Dry-run: only report -------------------------------------------------
if (dryRun) {
  console.log("[bump-version] --dry-run: no files written.");
  console.log(`  ${CARGO_TOML}  [package] version -> ${next}`);
  console.log(`  ${PACKAGE_JSON}          version -> ${next}`);
  if (newLock) console.log(`  ${CARGO_LOCK}  voicetypex        -> ${next}`);
  process.exit(0);
}

// --- Check the working tree (before writing) ------------------------------
if (!noGit) {
  if (git(["status", "--porcelain"]).trim()) {
    die("working tree not clean — commit/stash first, then release.");
  }
  if (git(["tag", "--list", `v${next}`]).trim()) {
    die(`tag v${next} already exists.`);
  }
}

// --- Write -----------------------------------------------------------------
writeFileSync(CARGO_TOML, newCargo);
writeFileSync(PACKAGE_JSON, newPkg);
if (newLock) writeFileSync(CARGO_LOCK, newLock);

if (noGit) {
  console.log("[bump-version] files bumped (--no-git: no commit/tag).");
  process.exit(0);
}

// --- Commit + annotated tag -----------------------------------------------
const files = [CARGO_TOML, PACKAGE_JSON];
if (newLock) files.push(CARGO_LOCK);
git(["add", ...files]);
git(["commit", "-m", `chore(release): v${next}`]);
git(["tag", "-a", `v${next}`, "-m", `v${next}`]);
console.log(`[bump-version] committed + tagged: v${next}`);
console.log("[bump-version] push with:  git push --follow-tags");

// --- Helper ----------------------------------------------------------------

// Reads the version from the [package] section — NOT from [dependencies]
// or similar, where dozens of `version = "…"` lines live.
function readPackageVersion(text) {
  let inPackage = false;
  for (const line of text.split("\n")) {
    if (/^\[package\]\s*$/.test(line)) {
      inPackage = true;
      continue;
    }
    if (inPackage && /^\[/.test(line)) break; // reached the next section
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
  die("could not replace [package] version in Cargo.toml");
  return text; // unreachable (die() exits), but keeps the linter happy
}

// Keyword bump: operates on the numeric core, discards any
// pre-release suffix. To finalize an -rc, specify the version explicitly.
function bump(version, kind) {
  const core = version.split("-")[0];
  const [maj, min, pat] = core.split(".").map((n) => parseInt(n, 10));
  if ([maj, min, pat].some((n) => Number.isNaN(n))) {
    die(`unparseable version: ${version}`);
  }
  if (kind === "major") return `${maj + 1}.0.0`;
  if (kind === "minor") return `${maj}.${min + 1}.0`;
  return `${maj}.${min}.${pat + 1}`;
}

function git(argv) {
  try {
    return execFileSync("git", argv, { encoding: "utf8" });
  } catch (e) {
    die(`git ${argv.join(" ")} failed: ${e.message}`);
  }
}
