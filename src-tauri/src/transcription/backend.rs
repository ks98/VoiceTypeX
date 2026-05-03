// SPDX-License-Identifier: GPL-3.0-or-later
//! Compile-Zeit-Detektion des aktiven Whisper-Backends.
//!
//! Das Backend wird zur Build-Zeit ueber Cargo-Features ausgewaehlt.
//! Default-Build (`cargo build`) ist `cpu` — kompiliert ohne
//! System-Abhaengigkeiten auf jedem Setup. Beschleunigte Varianten via
//! `--features fast-cpu | gpu-vulkan | gpu-cuda | gpu-metal | gpu-coreml`.
//!
//! Diese Funktion ist die einzige korrekte Quelle fuer "welches Backend
//! laeuft tatsaechlich" — sie kann nicht luegen, weil sie ueber `cfg!`
//! die Build-Konfiguration liest.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WhisperBackendInfo {
    /// Backend-Identifier: "cpu", "openblas", "vulkan", "cuda", "metal", "coreml".
    pub backend: &'static str,
    /// Kurze deutschsprachige Beschreibung fuer die UI.
    pub description: &'static str,
    /// Erwarteter Speedup-Faktor gegenueber `cpu`-Default. Konservative
    /// Schaetzung — echte Werte haengen vom Modell und der Hardware ab.
    pub expected_speedup: f32,
}

#[allow(clippy::needless_return, unreachable_code)]
pub fn active_backend() -> WhisperBackendInfo {
    // Reihenfolge: spezifischere/schnellere Backends zuerst. `return` in
    // jedem cfg-branch ist noetig, weil bei kombinierten Features mehrere
    // cfg-blocks aktiv sein koennten — dann waere fall-through falsch.
    #[cfg(feature = "gpu-cuda")]
    {
        return WhisperBackendInfo {
            backend: "cuda",
            description: "NVIDIA CUDA — sehr schnell, braucht CUDA-Toolkit + NVIDIA-GPU",
            expected_speedup: 10.0,
        };
    }
    #[cfg(feature = "gpu-vulkan")]
    {
        return WhisperBackendInfo {
            backend: "vulkan",
            description: "Vulkan — GPU-beschleunigt cross-platform (NVIDIA, AMD, Intel)",
            expected_speedup: 7.0,
        };
    }
    #[cfg(feature = "gpu-metal")]
    {
        return WhisperBackendInfo {
            backend: "metal",
            description: "Apple Metal — sehr schnell auf macOS-Geraeten",
            expected_speedup: 8.0,
        };
    }
    #[cfg(feature = "gpu-coreml")]
    {
        return WhisperBackendInfo {
            backend: "coreml",
            description: "Apple CoreML — Apple-Silicon optimiert",
            expected_speedup: 8.0,
        };
    }
    #[cfg(feature = "fast-cpu")]
    {
        return WhisperBackendInfo {
            backend: "openblas",
            description: "OpenBLAS — beschleunigte CPU-Math, keine GPU noetig",
            expected_speedup: 2.5,
        };
    }
    #[cfg(not(any(
        feature = "gpu-cuda",
        feature = "gpu-vulkan",
        feature = "gpu-metal",
        feature = "gpu-coreml",
        feature = "fast-cpu",
    )))]
    {
        WhisperBackendInfo {
            backend: "cpu",
            description: "CPU (kein BLAS, keine GPU) — default, langsamster Pfad",
            expected_speedup: 1.0,
        }
    }
}
