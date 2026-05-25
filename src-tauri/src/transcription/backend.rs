// SPDX-License-Identifier: GPL-3.0-or-later
//! Compile-time detection of the active Whisper backend.
//!
//! The backend is selected at build time via Cargo features. The
//! default build (`cargo build`) is `cpu` — compiles without system
//! dependencies on any setup. Accelerated variants via
//! `--features fast-cpu | gpu-vulkan | gpu-cuda | gpu-metal | gpu-coreml`.
//!
//! This function is the only correct source for "which backend is
//! actually running" — it cannot lie, because it reads the build
//! configuration via `cfg!`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct WhisperBackendInfo {
    /// Backend identifier: "cpu", "openblas", "vulkan", "cuda",
    /// "metal", "coreml".
    pub backend: &'static str,
    /// Short English description for the UI / Logs tab.
    pub description: &'static str,
    /// Expected speedup factor over the `cpu` default. A conservative
    /// estimate — real values depend on the model and the hardware.
    pub expected_speedup: f32,
}

#[allow(clippy::needless_return, unreachable_code)]
pub fn active_backend() -> WhisperBackendInfo {
    // Order: more specific/faster backends first. `return` in every
    // `cfg` branch is needed because with combined features several
    // `cfg` blocks could be active — fall-through would be wrong then.
    #[cfg(feature = "gpu-cuda")]
    {
        return WhisperBackendInfo {
            backend: "cuda",
            description: "NVIDIA CUDA — very fast, requires CUDA toolkit + NVIDIA GPU",
            expected_speedup: 10.0,
        };
    }
    #[cfg(feature = "gpu-vulkan")]
    {
        return WhisperBackendInfo {
            backend: "vulkan",
            description: "Vulkan — GPU-accelerated cross-platform (NVIDIA, AMD, Intel)",
            expected_speedup: 7.0,
        };
    }
    #[cfg(feature = "gpu-metal")]
    {
        return WhisperBackendInfo {
            backend: "metal",
            description: "Apple Metal — very fast on macOS devices",
            expected_speedup: 8.0,
        };
    }
    #[cfg(feature = "gpu-coreml")]
    {
        return WhisperBackendInfo {
            backend: "coreml",
            description: "Apple CoreML — Apple Silicon optimized",
            expected_speedup: 8.0,
        };
    }
    #[cfg(feature = "fast-cpu")]
    {
        return WhisperBackendInfo {
            backend: "openblas",
            description: "OpenBLAS — accelerated CPU math, no GPU needed",
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
            description: "CPU (no BLAS, no GPU) — default, slowest path",
            expected_speedup: 1.0,
        }
    }
}
