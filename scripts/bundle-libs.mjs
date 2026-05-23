// SPDX-License-Identifier: GPL-3.0-or-later
//
// Kopiert die zur Laufzeit benoetigten Shared-Libs (Linux: `libllama.so*`,
// `libggml*.so*`; Windows: `llama.dll`, `ggml*.dll`) von Cargo's
// target/{debug,release}/-Verzeichnis nach `src-tauri/resources/lib/`.
// Wird ueber `beforeBundleCommand` in `tauri.conf.json` getriggert, laeuft
// also NACH `cargo build` und VOR `tauri-bundler`.
//
// Tauri-bundler picked die Files via `bundle.resources`-Glob auf und legt
// sie ins finale Bundle.
//
// - **Linux**: Files landen in `$ORIGIN/resources/lib/`. Der Binary findet
//   sie zur Laufzeit via rpath-Kaskade (`src-tauri/build.rs`).
// - **Windows**: Files landen in `$INSTDIR\resources\lib\`. Der Windows-
//   DLL-Loader sucht dort **nicht** automatisch — siehe NSIS-Hook in
//   `docs/PLATFORMS.md` (Erster Windows-Bundle-Build verifiziert das).

import { existsSync, mkdirSync, readdirSync, copyFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const RESOURCES_LIB = join(REPO_ROOT, "src-tauri", "resources", "lib");

const IS_WINDOWS = process.platform === "win32";

// Linux: `libllama.so`, `libllama.so.0`, `libllama.so.0.0.0`,
//        `libggml.so`, `libggml-cpu.so`, `libggml-vulkan.so`, `libggml-base.so`.
// Windows: `llama.dll`, `ggml.dll`, `ggml-cpu.dll`, `ggml-vulkan.dll`,
//          `ggml-base.dll`. Keine Versions-Suffixe auf Win.
const PATTERNS = IS_WINDOWS
  ? [/^ggml(-[a-z0-9_]+)?\.dll$/, /^llama\.dll$/]
  : [/^libggml(-[a-z0-9_]+)?\.so(\.[\d.]+)?$/, /^libllama\.so(\.[\d.]+)?$/];

const PROFILE_MARKER = IS_WINDOWS ? /^llama\.dll$/ : /^libllama\.so/;

// Reihenfolge: zuerst release, dann debug. Release hat Vorrang, wenn
// beides existiert (Bundle-Builds laufen typisch im release-Profil).
const SOURCE_DIRS = [
  join(REPO_ROOT, "src-tauri", "target", "release"),
  join(REPO_ROOT, "src-tauri", "target", "debug"),
];

function findFirstSource() {
  for (const dir of SOURCE_DIRS) {
    if (!existsSync(dir)) continue;
    // Brauchen das llama-Marker-File damit wir das richtige Profil
    // wissen — kein Profil ohne llama.cpp gebaut.
    const entries = readdirSync(dir);
    if (entries.some((n) => PROFILE_MARKER.test(n))) {
      return dir;
    }
  }
  return null;
}

const source = findFirstSource();
if (!source) {
  const marker = IS_WINDOWS ? "llama.dll" : "libllama.so*";
  console.warn(
    `[bundle-libs] Keine target/{release,debug}/${marker} gefunden — ` +
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
