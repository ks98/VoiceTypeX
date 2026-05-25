// SPDX-License-Identifier: GPL-3.0-or-later
//! Runtime hardware detection.
//!
//! Detects at runtime which acceleration backends **would** be
//! available on the user's hardware — even when the current build
//! hasn't enabled them. This lets onboarding concretely recommend
//! "download the Vulkan variant for 5x speedup".
//!
//! The detection strategy is deliberately simple: we look at the
//! presence of libraries and devices, not at "would it actually work".
//! False positives are acceptable — the user notices on variant
//! download and switches back.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HardwareReport {
    pub os: &'static str,
    pub cpu_logical_cores: u32,

    /// libopenblas available at runtime? Linux: probe standard paths.
    pub has_openblas: bool,
    /// libvulkan available at runtime? **Note**: this only checks
    /// whether the loader is installed, not whether a physical device
    /// is active. A VM with virtio-gpu has libvulkan but falls back to
    /// llvmpipe (software rendering). Real device enumeration needs
    /// the `ash` crate — a phase-3c topic.
    pub has_vulkan: bool,
    /// NVIDIA GPU present? Linux: /dev/nvidia* or libcuda.so.
    pub has_nvidia_gpu: bool,
    /// AMD GPU present? Linux: /dev/dri + amdgpu driver or
    /// libamdhip64.
    pub has_amd_gpu: bool,
    /// Apple Silicon (M1/M2/...)?
    pub is_apple_silicon: bool,

    /// Total RAM in GB (rounded to 1 decimal). `0.0` = detection not
    /// implemented for this OS (currently: Windows).
    pub total_ram_gb: f32,
    /// Currently available RAM in GB (MemAvailable on Linux). `0.0` =
    /// detection not implemented.
    pub available_ram_gb: f32,

    /// Which build bundle would be optimal for this hardware?
    /// Values: "cpu", "openblas", "vulkan", "cuda", "metal".
    pub recommended_variant: &'static str,

    /// Expected speedup factor of `recommended_variant` over CPU-only.
    pub recommended_speedup: f32,
}

pub fn detect() -> HardwareReport {
    let cpu_logical_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4);

    let has_openblas = detect_openblas();
    let has_vulkan = detect_vulkan();
    let has_nvidia_gpu = detect_nvidia_gpu();
    let has_amd_gpu = detect_amd_gpu();
    let is_apple_silicon = detect_apple_silicon();
    let (total_ram_gb, available_ram_gb) = detect_ram_gb();

    let (recommended_variant, recommended_speedup) =
        pick_recommendation(has_openblas, has_vulkan, has_nvidia_gpu, is_apple_silicon);

    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    };

    HardwareReport {
        os,
        cpu_logical_cores,
        has_openblas,
        has_vulkan,
        has_nvidia_gpu,
        has_amd_gpu,
        is_apple_silicon,
        total_ram_gb,
        available_ram_gb,
        recommended_variant,
        recommended_speedup,
    }
}

/// RAM detection. Linux: `/proc/meminfo` parser (MemTotal +
/// MemAvailable). On other OSes currently (0.0, 0.0) — phase 3c will
/// pull in the `sysinfo` crate for cross-platform RAM/VRAM detection.
fn detect_ram_gb() -> (f32, f32) {
    #[cfg(target_os = "linux")]
    {
        let content = match std::fs::read_to_string("/proc/meminfo") {
            Ok(c) => c,
            Err(_) => return (0.0, 0.0),
        };
        let mut total_kb: u64 = 0;
        let mut available_kb: u64 = 0;
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total_kb = rest
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                available_kb = rest
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
            }
        }
        let to_gb = |kb: u64| (kb as f32) / 1024.0 / 1024.0;
        (
            (to_gb(total_kb) * 10.0).round() / 10.0,
            (to_gb(available_kb) * 10.0).round() / 10.0,
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        (0.0, 0.0)
    }
}

fn pick_recommendation(
    has_openblas: bool,
    has_vulkan: bool,
    has_nvidia: bool,
    is_apple_silicon: bool,
) -> (&'static str, f32) {
    // Priority: Apple Silicon > NVIDIA CUDA > Vulkan > OpenBLAS > CPU.
    if is_apple_silicon {
        return ("metal", 8.0);
    }
    if has_nvidia {
        return ("cuda", 10.0);
    }
    if has_vulkan {
        return ("vulkan", 7.0);
    }
    if has_openblas {
        return ("openblas", 2.5);
    }
    ("cpu", 1.0)
}

#[cfg(target_os = "linux")]
fn detect_openblas() -> bool {
    library_present(&["libopenblas.so.0", "libopenblas.so", "libblas.so.3"])
}

#[cfg(target_os = "windows")]
fn detect_openblas() -> bool {
    // OpenBLAS on Windows ships with the bundle (statically or
    // alongside).
    true
}

#[cfg(target_os = "macos")]
fn detect_openblas() -> bool {
    // macOS has Accelerate.framework — equivalent.
    true
}

#[cfg(target_os = "linux")]
fn detect_vulkan() -> bool {
    library_present(&["libvulkan.so.1", "libvulkan.so"])
}

#[cfg(target_os = "windows")]
fn detect_vulkan() -> bool {
    // The Vulkan runtime ships with GPU drivers (NVIDIA, AMD, Intel).
    // Heuristic: check whether vulkan-1.dll is in System32.
    std::path::Path::new("C:\\Windows\\System32\\vulkan-1.dll").exists()
}

#[cfg(target_os = "macos")]
fn detect_vulkan() -> bool {
    // MoltenVK exists, but Metal is the better path.
    false
}

#[cfg(target_os = "linux")]
fn detect_nvidia_gpu() -> bool {
    std::path::Path::new("/dev/nvidia0").exists()
        || std::path::Path::new("/proc/driver/nvidia/version").exists()
}

#[cfg(target_os = "windows")]
fn detect_nvidia_gpu() -> bool {
    std::path::Path::new("C:\\Windows\\System32\\nvcuda.dll").exists()
}

#[cfg(target_os = "macos")]
fn detect_nvidia_gpu() -> bool {
    // Apple no longer sells Macs with NVIDIA GPUs.
    false
}

#[cfg(target_os = "linux")]
fn detect_amd_gpu() -> bool {
    // amdgpu driver module loaded?
    std::fs::read_to_string("/proc/modules")
        .map(|m| m.contains("amdgpu"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn detect_amd_gpu() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn detect_apple_silicon() -> bool {
    cfg!(target_arch = "aarch64")
}

#[cfg(not(target_os = "macos"))]
fn detect_apple_silicon() -> bool {
    false
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn library_present(candidates: &[&str]) -> bool {
    let search_paths = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
        "/lib/x86_64-linux-gnu",
        "/lib64",
        "/lib",
        "/usr/local/lib",
    ];
    for path in &search_paths {
        for name in candidates {
            if std::path::Path::new(path).join(name).exists() {
                return true;
            }
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn library_present(_candidates: &[&str]) -> bool {
    // DLL lookup is more involved on Windows; the specific paths are
    // hardcoded above in `detect_openblas`/`detect_vulkan`.
    false
}
