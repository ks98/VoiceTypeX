// SPDX-License-Identifier: GPL-3.0-or-later
//
// Kopiert die zur Laufzeit benoetigten Shared-Libs (Linux: `libllama.so*`,
// `libggml*.so*`; Windows: `llama.dll`, `ggml*.dll`) nach
// `src-tauri/resources/lib/`, damit der tauri-bundler sie via
// `bundle.resources` ins finale Bundle packt. Getriggert ueber
// `beforeBundleCommand` in `tauri.conf.json` — laeuft NACH `cargo build`,
// VOR dem tauri-bundler.
//
// Quelle ist das KANONISCHE Output-Verzeichnis von llama-cpp-sys-2
// (`target/<profile>/build/llama-cpp-sys-2-<hash>/out/lib`). Dort liegt
// immer die vollstaendige, korrekte Symlink-Kette
// (`libfoo.so -> libfoo.so.0 -> libfoo.so.0.9.11`). Das frueher genutzte
// Top-Level `target/<profile>/` enthaelt durch das Zusammenspiel von
// `clean-dangling-libs` + gecachtem build.rs teils *dangling* Symlinks —
// `copyFileSync` folgt ihnen ins Leere (ENOENT). Daher NICHT von dort
// quellen.
//
// Symlinks werden ALS Symlinks uebernommen (eine reale Datei + Kette),
// damit der Runtime-Loader die SONAME (z.B. `libllama.so.0`) aufloest und
// das Bundle nicht jede Lib dreifach enthaelt.
//
// - **Linux**: Files landen in `$ORIGIN/resources/lib/`, gefunden via
//   rpath-Kaskade (`src-tauri/build.rs`).
// - **Windows**: `$INSTDIR\resources\lib\` — der DLL-Loader sucht dort
//   nicht automatisch (NSIS-Hook, siehe `docs/PLATFORMS.md`). Das
//   Windows-Release ist derzeit zurueckgestellt (ks98/voicetypex#1).

import {
  existsSync,
  mkdirSync,
  readdirSync,
  copyFileSync,
  lstatSync,
  readlinkSync,
  symlinkSync,
  rmSync,
  statSync,
} from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const RESOURCES_LIB = join(REPO_ROOT, "src-tauri", "resources", "lib");

const IS_WINDOWS = process.platform === "win32";

const PATTERNS = IS_WINDOWS
  ? [/^ggml(-[a-z0-9_]+)?\.dll$/, /^llama\.dll$/]
  : [/^libggml(-[a-z0-9_]+)?\.so(\.[\d.]+)?$/, /^libllama\.so(\.[\d.]+)?$/];

// Marker-Datei, an der wir das richtige out-Verzeichnis erkennen.
const MARKER = IS_WINDOWS
  ? /^llama\.dll$/
  : /^libllama\.so(\.[\d.]+)?$/;

// Unterordner im build-out, in denen die Libs liegen (Plattform-abhaengig).
const OUT_SUBDIRS = IS_WINDOWS
  ? ["out/build/bin", "out/bin", "out/lib"]
  : ["out/lib"];

function markerMtime(dir) {
  let newest = 0;
  for (const n of readdirSync(dir)) {
    if (!MARKER.test(n)) continue;
    try {
      newest = Math.max(newest, lstatSync(join(dir, n)).mtimeMs);
    } catch {
      /* ignore */
    }
  }
  return newest;
}

// Neuestes llama-cpp-sys-2-out-Verzeichnis mit den Libs (release vor debug).
function findSourceDir() {
  const found = [];
  for (const profile of ["release", "debug"]) {
    const buildRoot = join(REPO_ROOT, "src-tauri", "target", profile, "build");
    if (!existsSync(buildRoot)) continue;
    for (const entry of readdirSync(buildRoot)) {
      if (!entry.startsWith("llama-cpp-sys-2-")) continue;
      for (const sub of OUT_SUBDIRS) {
        const dir = join(buildRoot, entry, ...sub.split("/"));
        if (existsSync(dir) && readdirSync(dir).some((n) => MARKER.test(n))) {
          found.push(dir);
        }
      }
    }
  }
  if (found.length === 0) return null;
  found.sort((a, b) => markerMtime(b) - markerMtime(a));
  return found[0];
}

const source = findSourceDir();
if (!source) {
  const marker = IS_WINDOWS ? "llama.dll" : "libllama.so*";
  console.warn(
    `[bundle-libs] Kein llama-cpp-sys-2 out-Verzeichnis mit ${marker} ` +
      "gefunden — Cargo-Build muss vorher laufen. Skript ist No-Op.",
  );
  process.exit(0);
}

mkdirSync(RESOURCES_LIB, { recursive: true });

let copied = 0;
let linked = 0;
for (const name of readdirSync(source)) {
  if (!PATTERNS.some((p) => p.test(name))) continue;
  const src = join(source, name);
  const dst = join(RESOURCES_LIB, name);
  try {
    const st = lstatSync(src);
    if (st.isSymbolicLink()) {
      // Symlink-Kette erhalten: relatives Ziel uebernehmen.
      rmSync(dst, { force: true });
      symlinkSync(readlinkSync(src), dst);
      linked += 1;
    } else if (st.isFile()) {
      // Reale Datei: nur kopieren, wenn neu/geaendert (Groesse).
      if (existsSync(dst) && !lstatSync(dst).isSymbolicLink() && statSync(dst).size === st.size) {
        continue;
      }
      rmSync(dst, { force: true });
      copyFileSync(src, dst);
      copied += 1;
    }
  } catch (e) {
    console.error(`[bundle-libs] ${name} fehlgeschlagen:`, e);
    process.exit(1);
  }
}

console.log(
  `[bundle-libs] ${copied} Libs + ${linked} Symlinks aus ${source} nach ${RESOURCES_LIB}.`,
);
