// SPDX-License-Identifier: GPL-3.0-or-later
//! Build script.
//!
//! - Calls `tauri_build::build()` for the standard Tauri setup steps.
//! - On Linux, adds rpath entries to the binary so the shared libs
//!   needed at runtime (`libllama.so`, `libggml*.so` from the
//!   llama-cpp-sys-2 build with the `dynamic-link` feature) are still
//!   found after Tauri bundling.
//!
//! We set several rpath entries as a fallback cascade, because the
//! Tauri bundler places the resources at different paths depending on
//! the bundle format (.deb/.rpm/AppImage). Linker behavior:
//! non-existent paths are simply ignored at runtime, no error.

fn main() {
    tauri_build::build();

    #[cfg(target_os = "linux")]
    {
        // 1. `$ORIGIN` — libs right next to the binary (classic
        //    portable-install variant, how the AppImage is laid out).
        // 2. `$ORIGIN/lib` — one level below the binary (Tauri puts
        //    bundle.resources here in some cases).
        // 3. `$ORIGIN/../lib/voicetypex` — FHS Linux, .deb standard:
        //    binary in /usr/bin/, libs in /usr/lib/voicetypex/.
        // 4. `$ORIGIN/../lib` — generic LSB layout for .rpm.
        // Multiple entries do no harm; the dynamic linker tries them
        // in order and takes the first hit.
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/lib");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/voicetypex");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib");
    }
}
