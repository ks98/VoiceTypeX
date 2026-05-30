// SPDX-License-Identifier: GPL-3.0-or-later
//
// Platform detection helper. Wraps `platform()` from @tauri-apps/plugin-os
// so feature gates have a single source of truth. The embedded llama-cpp-2
// LLM is Linux/macOS-only — on Windows it collides with whisper's ggml at
// link time (issue #1), so the whole embedded-LLM UI is hidden there.
//
// Lazy + guarded: `platform()` reads a Tauri-injected global that is absent
// under vitest / non-Tauri contexts, so a throw degrades to a safe default
// ("unknown") instead of crashing at module load.

import { platform } from "@tauri-apps/plugin-os";

let cached: string | null = null;

function currentPlatform(): string {
  if (cached === null) {
    try {
      cached = platform();
    } catch {
      cached = "unknown";
    }
  }
  return cached;
}

/**
 * True on Windows, where the embedded llama-cpp-2 LLM is not compiled in
 * (issue #1). Callers use this to hide embedded-LLM UI and steer the user
 * to a cloud provider or a self-installed Ollama daemon instead.
 */
export function isWindows(): boolean {
  return currentPlatform() === "windows";
}
