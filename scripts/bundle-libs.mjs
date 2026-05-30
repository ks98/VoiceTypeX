// SPDX-License-Identifier: GPL-3.0-or-later
//
// Copies the shared libs needed at runtime (Linux: `libllama.so*`,
// `libggml*.so*`; Windows: `llama.dll`, `ggml*.dll`) to
// `src-tauri/resources/lib/`, so the tauri-bundler packs them into the
// final bundle via `bundle.resources`. Triggered via
// `beforeBundleCommand` in `tauri.conf.json` — runs AFTER `cargo build`,
// BEFORE the tauri-bundler.
//
// On Windows the embedded llama-cpp-2 is not compiled (Issue #1) —
// whisper-rs-sys links its ggml statically, so there is no
// `llama.dll`/`ggml*.dll`. The script then finds no source and is a
// clean no-op (Exit 0). Real copying only happens on Linux/macOS.
//
// The source is the CANONICAL output directory of llama-cpp-sys-2
// (`target/<profile>/build/llama-cpp-sys-2-<hash>/out/lib`). It always
// holds the complete, correct symlink chain
// (`libfoo.so -> libfoo.so.0 -> libfoo.so.0.9.11`). The previously used
// top-level `target/<profile>/` contains, due to the interplay of
// `clean-dangling-libs` + a cached build.rs, partly *dangling* symlinks —
// `copyFileSync` follows them into the void (ENOENT). Therefore do NOT
// source from there.
//
// Symlinks are carried over AS symlinks (one real file + chain), so the
// runtime loader resolves the SONAME (e.g. `libllama.so.0`) and the
// bundle does not contain every lib three times.
//
// - **Linux**: Files land in `$ORIGIN/resources/lib/`, found via the
//   rpath cascade (`src-tauri/build.rs`).
// - **Windows**: `$INSTDIR\resources\lib\` — the DLL loader does not
//   search there automatically (NSIS hook, see `docs/PLATFORMS.md`). The
//   Windows release is currently deferred (ks98/voicetypex#1).

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

// Marker file by which we recognize the correct out directory.
const MARKER = IS_WINDOWS
  ? /^llama\.dll$/
  : /^libllama\.so(\.[\d.]+)?$/;

// Subdirectories in the build-out where the libs reside (platform-dependent).
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

// Newest llama-cpp-sys-2-out directory with the libs (release before debug).
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
    `[bundle-libs] No llama-cpp-sys-2 out directory with ${marker} ` +
      "found — cargo build must run first. Script is a no-op.",
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
      // Preserve the symlink chain: carry over the relative target.
      rmSync(dst, { force: true });
      symlinkSync(readlinkSync(src), dst);
      linked += 1;
    } else if (st.isFile()) {
      // Real file: only copy if new/changed (size).
      if (existsSync(dst) && !lstatSync(dst).isSymbolicLink() && statSync(dst).size === st.size) {
        continue;
      }
      rmSync(dst, { force: true });
      copyFileSync(src, dst);
      copied += 1;
    }
  } catch (e) {
    console.error(`[bundle-libs] ${name} failed:`, e);
    process.exit(1);
  }
}

console.log(
  `[bundle-libs] ${copied} libs + ${linked} symlinks from ${source} to ${RESOURCES_LIB}.`,
);
