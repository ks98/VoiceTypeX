// SPDX-License-Identifier: GPL-3.0-or-later
//! Active Whisper backend reporting.
//!
//! The backend is *selected* at build time via Cargo features (default
//! build is `cpu`; accelerated variants via `--features fast-cpu |
//! gpu-vulkan | gpu-cuda | gpu-metal | gpu-coreml`). But the build flag is
//! only the INTENT — whether a GPU is actually usable at runtime is decided
//! by ggml when it enumerates devices. So for the Vulkan build we verify at
//! runtime (`whisper_rs::vulkan::list_devices`) and report the REAL backend,
//! which makes a silent CPU fallback (Vulkan build, but no usable Vulkan
//! device → whisper.cpp runs on CPU) visible instead of always claiming
//! "vulkan". The other GPU backends have no equivalent runtime probe in
//! whisper-rs, so they still report their build-time intent.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WhisperBackendInfo {
    /// Backend identifier: "cpu", "openblas", "vulkan", "cuda",
    /// "metal", "coreml".
    pub backend: String,
    /// Short English description for the UI / Logs tab. For the Vulkan
    /// runtime path this includes the detected device name.
    pub description: String,
    /// Expected speedup factor over the `cpu` default. A conservative
    /// estimate — real values depend on the model and the hardware.
    pub expected_speedup: f32,
}

impl WhisperBackendInfo {
    fn new(backend: &str, description: impl Into<String>, expected_speedup: f32) -> Self {
        Self {
            backend: backend.to_string(),
            description: description.into(),
            expected_speedup,
        }
    }
}

#[allow(clippy::needless_return, unreachable_code)]
pub fn active_backend() -> WhisperBackendInfo {
    // Order: more specific/faster backends first. `return` in every
    // `cfg` branch is needed because with combined features several
    // `cfg` blocks could be active — fall-through would be wrong then.
    #[cfg(feature = "gpu-cuda")]
    {
        return WhisperBackendInfo::new(
            "cuda",
            "NVIDIA CUDA — very fast, requires CUDA toolkit + NVIDIA GPU",
            10.0,
        );
    }
    #[cfg(feature = "gpu-vulkan")]
    {
        return vulkan_runtime_backend();
    }
    #[cfg(feature = "gpu-metal")]
    {
        return WhisperBackendInfo::new("metal", "Apple Metal — very fast on macOS devices", 8.0);
    }
    #[cfg(feature = "gpu-coreml")]
    {
        return WhisperBackendInfo::new("coreml", "Apple CoreML — Apple Silicon optimized", 8.0);
    }
    #[cfg(feature = "fast-cpu")]
    {
        return WhisperBackendInfo::new(
            "openblas",
            "OpenBLAS — accelerated CPU math, no GPU needed",
            2.5,
        );
    }
    #[cfg(not(any(
        feature = "gpu-cuda",
        feature = "gpu-vulkan",
        feature = "gpu-metal",
        feature = "gpu-coreml",
        feature = "fast-cpu",
    )))]
    {
        WhisperBackendInfo::new("cpu", "CPU (no BLAS, no GPU) — default, slowest path", 1.0)
    }
}

/// Vulkan build: ask ggml which Vulkan devices it can actually see. An
/// empty list means whisper.cpp will silently run on CPU despite the
/// Vulkan build — report that truthfully so a "why is it slow?" can be
/// diagnosed instead of trusting the build flag.
#[cfg(feature = "gpu-vulkan")]
fn vulkan_runtime_backend() -> WhisperBackendInfo {
    let devices = whisper_rs::vulkan::list_devices();
    match devices.first() {
        Some(dev) => {
            let vram_mb = dev.vram.total / (1024 * 1024);
            WhisperBackendInfo::new(
                "vulkan",
                format!("Vulkan — {} ({vram_mb} MB VRAM)", dev.name),
                7.0,
            )
        }
        None => WhisperBackendInfo::new(
            "cpu",
            "Vulkan build, but no Vulkan device detected — running on CPU (slow). \
             Check GPU drivers / libvulkan.",
            1.0,
        ),
    }
}

/// Log the real (runtime-resolved) Whisper backend once at startup, so a
/// silent CPU fallback on a Vulkan build is visible in the Logs tab.
pub fn log_active_backend() {
    let b = active_backend();
    tracing::info!(
        backend = %b.backend,
        expected_speedup = b.expected_speedup,
        "Whisper backend: {}",
        b.description
    );
}
