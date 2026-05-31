// SPDX-License-Identifier: GPL-3.0-or-later
//! Build script.
//!
//! - Calls `tauri_build::build()` for the standard Tauri setup steps.
//! - On Linux, sets the rpath so the shared libs needed at runtime
//!   (`libllama.so`, `libggml*.so` from the llama-cpp-sys-2 build with
//!   the `dynamic-link` feature) are found after Tauri bundling.
//!
//! Two specifics, both verified against a real .rpm install + an `ld.so`
//! repro (see docs/PLATFORMS.md):
//!   * The bundled libs carry NO rpath of their own and need each other
//!     transitively (llama -> ggml -> ggml-{cpu,vulkan,base}). DT_RUNPATH
//!     (the linker default = new dtags) is NOT inherited by transitive
//!     deps, so a RUNPATH resolves libllama but then fails on
//!     libggml.so.0. We therefore force DT_RPATH (`--disable-new-dtags`),
//!     which IS inherited down the whole chain — valid here because none
//!     of the bundled libs declare their own RUNPATH.
//!   * The deb/rpm bundler installs `bundle.resources` under
//!     `/usr/lib/<productName>/`, i.e. `/usr/lib/VoiceTypeX/...` — the
//!     verbatim productName, NOT the binary name `voicetypex`. The rpath
//!     must hit that exact path. Non-existent cascade entries are simply
//!     ignored at runtime, no error.

fn main() {
    tauri_build::build();

    #[cfg(target_os = "linux")]
    {
        // Emit DT_RPATH instead of DT_RUNPATH so the entries below are
        // inherited by libllama's transitive ggml deps (see module doc).
        println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags");

        // rpath cascade (tried in order, missing dirs ignored):
        // 1. deb/rpm — Tauri resource root is the productName "VoiceTypeX";
        //    binary in /usr/bin, so $ORIGIN/../lib = /usr/lib. Update this
        //    if productName ever changes.
        // 2. AppImage — linuxdeploy deploys the NEEDED libs to AppDir/usr/lib.
        // 3. portable/dev — libs right next to the binary.
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/VoiceTypeX/resources/lib");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    }
}
