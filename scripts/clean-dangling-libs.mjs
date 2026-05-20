// SPDX-License-Identifier: GPL-3.0-or-later
//
// Räumt vor jedem Build die `libggml*.so*` und `libllama.so*` aus
// `src-tauri/target/debug/` (+ deps/, examples/).
//
// Hintergrund: llama-cpp-sys-2 0.1.146's build.rs hat einen TOC/TOU-
// Bug — `Path::exists()` folgt Symlinks und liefert false fuer
// dangling Links, `std::fs::hard_link()` schlaegt aber fehl, weil der
// Symlink-Eintrag noch da ist. Resultat: `Os { code: 17, kind:
// AlreadyExists }`-Panic.
//
// Hooks in package.json: `predev` und `prebuild`. Pflegt sich selbst
// solange dieses Script existiert.
//
// Cross-Platform: pure Node, kein `find`. Auf Windows matched das
// Pattern nichts (dort heisst's `.dll`, der Bug existiert auch nicht),
// also harmloser No-Op.

import { readdirSync, unlinkSync } from "node:fs";
import { join } from "node:path";

const TARGETS = [
  "src-tauri/target/debug",
  "src-tauri/target/debug/deps",
  "src-tauri/target/debug/examples",
  "src-tauri/target/release",
  "src-tauri/target/release/deps",
  "src-tauri/target/release/examples",
];

const PATTERNS = [
  // libggml.so, libggml.so.0, libggml.so.0.9.11, libggml-cpu.so, ...
  /^libggml(-[a-z0-9_]+)?\.so(\.[\d.]+)?$/,
  // libllama.so, libllama.so.0, libllama.so.0.0.0
  /^libllama\.so(\.[\d.]+)?$/,
];

function cleanDir(dir) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    // Verzeichnis existiert nicht (erster Build, oder release-target
    // noch nie gebaut) → kein Problem, einfach weiter.
    return 0;
  }
  let removed = 0;
  for (const name of entries) {
    if (!PATTERNS.some((p) => p.test(name))) continue;
    const full = join(dir, name);
    try {
      unlinkSync(full);
      removed += 1;
    } catch {
      // Datei verschwand zwischen readdir und unlink — nicht fatal.
    }
  }
  return removed;
}

let total = 0;
for (const dir of TARGETS) {
  total += cleanDir(dir);
}

if (total > 0) {
  console.log(
    `[clean-dangling-libs] ${total} stale ggml/llama-Library-Eintrag(e) entfernt.`,
  );
}
