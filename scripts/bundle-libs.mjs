// SPDX-License-Identifier: GPL-3.0-or-later
//
// Kopiert die zur Laufzeit benoetigten Shared-Libs (`libllama.so*`,
// `libggml*.so*`) von Cargo's target/{debug,release}/-Verzeichnis nach
// `src-tauri/resources/lib/`. Wird ueber `beforeBundleCommand` in
// `tauri.conf.json` getriggered, laeuft also NACH `cargo build` und VOR
// `tauri-bundler`.
//
// Tauri-bundler picked die Files via `bundle.resources`-Glob auf und
// legt sie ins finale Bundle. Zur Laufzeit findet sie der Linker via
// rpath (siehe `src-tauri/build.rs`).
//
// Cross-platform: Linux/macOS aus target/{debug,release}/ → resources/lib/.
// Windows: .dll-Files heissen anders, aber Bundle-Layout ist auch
// anders (Tauri legt sie automatisch neben die .exe). Wir lassen den
// Windows-Pfad als TODO — aktuell ohne dynamic-link-Win32-Build untestbar.

import { existsSync, mkdirSync, readdirSync, copyFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const RESOURCES_LIB = join(REPO_ROOT, "src-tauri", "resources", "lib");

const PATTERNS = [
  /^libggml(-[a-z0-9_]+)?\.so(\.[\d.]+)?$/,
  /^libllama\.so(\.[\d.]+)?$/,
];

// Reihenfolge: zuerst release, dann debug. Release hat Vorrang, wenn
// beides existiert (Bundle-Builds laufen typisch im release-Profil).
const SOURCE_DIRS = [
  join(REPO_ROOT, "src-tauri", "target", "release"),
  join(REPO_ROOT, "src-tauri", "target", "debug"),
];

function findFirstSource() {
  for (const dir of SOURCE_DIRS) {
    if (!existsSync(dir)) continue;
    // Brauchen mindestens libllama.so* damit wir das richtige Profil
    // wissen — kein Profil ohne llama.cpp gebaut.
    const entries = readdirSync(dir);
    if (entries.some((n) => /^libllama\.so/.test(n))) {
      return dir;
    }
  }
  return null;
}

const source = findFirstSource();
if (!source) {
  console.warn(
    "[bundle-libs] Keine target/{release,debug}/libllama.so* gefunden — " +
      "Cargo-Build muss vorher laufen. Skript ist No-Op.",
  );
  process.exit(0);
}

mkdirSync(RESOURCES_LIB, { recursive: true });

let copied = 0;
for (const name of readdirSync(source)) {
  if (!PATTERNS.some((p) => p.test(name))) continue;
  const src = join(source, name);
  const dst = join(RESOURCES_LIB, name);
  try {
    // Wenn dst schon existiert mit gleicher Groesse: skip (Cache).
    if (
      existsSync(dst) &&
      statSync(dst).size === statSync(src).size
    ) {
      continue;
    }
    copyFileSync(src, dst);
    copied += 1;
  } catch (e) {
    console.error(`[bundle-libs] copy ${name} fehlgeschlagen:`, e);
    process.exit(1);
  }
}

console.log(
  `[bundle-libs] ${copied} Libs aus ${source} nach ${RESOURCES_LIB} kopiert.`,
);
