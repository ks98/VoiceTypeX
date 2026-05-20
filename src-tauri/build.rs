// SPDX-License-Identifier: GPL-3.0-or-later
//! Build-Script.
//!
//! - Ruft `tauri_build::build()` fuer die Standard-Tauri-Setup-Schritte.
//! - Fuegt unter Linux rpath-Entries zum Binary, damit die zur Laufzeit
//!   benoetigten Shared-Libs (`libllama.so`, `libggml*.so` vom
//!   llama-cpp-sys-2-Build mit `dynamic-link`-Feature) auch nach dem
//!   Tauri-Bundling gefunden werden.
//!
//! Wir setzen mehrere rpath-Entries als Fallback-Kaskade, weil der
//! Tauri-Bundler abhaengig vom Bundle-Format (.deb/.rpm/AppImage)
//! die Resources an unterschiedliche Pfade legt. Linker-Verhalten:
//! nicht-existierende Pfade werden zur Laufzeit einfach ignoriert,
//! kein Fehler.

fn main() {
    tauri_build::build();

    #[cfg(target_os = "linux")]
    {
        // 1. `$ORIGIN` — Libs direkt neben der Binary (klassische
        //    Portable-Install-Variante, AppImage haengt so).
        // 2. `$ORIGIN/lib` — eine Ebene unter dem Binary (Tauri legt
        //    bundle.resources teils so ab).
        // 3. `$ORIGIN/../lib/voicetypex` — FHS-Linux, .deb-Standard:
        //    Binary in /usr/bin/, Libs in /usr/lib/voicetypex/.
        // 4. `$ORIGIN/../lib` — Generic LSB-Layout fuer .rpm.
        // Mehrfache Eintraege schaden nicht; der dynamische Linker
        // probiert sie in Reihenfolge, nimmt den ersten Treffer.
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/lib");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/voicetypex");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib");
    }
}
