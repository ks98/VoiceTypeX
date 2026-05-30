// SPDX-License-Identifier: GPL-3.0-or-later
//
// Cleans up the `libggml*.so*` and `libllama.so*` files from
// `src-tauri/target/debug/` (+ deps/, examples/) before every build.
//
// Background: llama-cpp-sys-2 0.1.146's build.rs has a TOC/TOU
// bug — `Path::exists()` follows symlinks and returns false for
// dangling links, but `std::fs::hard_link()` fails because the
// symlink entry is still there. Result: `Os { code: 17, kind:
// AlreadyExists }` panic.
//
// Hooks in package.json: `predev` and `prebuild`. Maintains itself
// as long as this script exists.
//
// Cross-platform: pure Node, no `find`. On Windows the pattern
// matches nothing (there they're named `.dll`, and the bug doesn't
// exist either), so it's a harmless no-op.

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
    // Directory doesn't exist (first build, or release target
    // never built yet) → no problem, just continue.
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
      // File vanished between readdir and unlink — not fatal.
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
    `[clean-dangling-libs] removed ${total} stale ggml/llama library entr${total === 1 ? "y" : "ies"}.`,
  );
}
