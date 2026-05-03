// SPDX-License-Identifier: GPL-3.0-or-later
//! Runtime-Hardware-Detection.
//!
//! Erkennt zur Laufzeit, welche Beschleunigungs-Backends auf der
//! User-Hardware verfuegbar **waeren** — auch wenn der aktuelle Build sie
//! nicht aktiviert hat. Damit kann das Onboarding dem User konkret
//! empfehlen "Lade die Vulkan-Variante fuer 5x Speedup".
//!
//! Detection-Strategie ist bewusst simpel: wir schauen auf die Anwesenheit
//! von Bibliotheken und Devices, nicht auf "wuerde es wirklich
//! funktionieren". Falsche Positives sind akzeptabel — der User merkt das
//! beim Variant-Download und wechselt zurueck.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HardwareReport {
    pub os: &'static str,
    pub cpu_logical_cores: u32,

    /// libopenblas zur Laufzeit verfuegbar? Linux: pruefe Standard-Pfade.
    pub has_openblas: bool,
    /// libvulkan zur Laufzeit verfuegbar?
    pub has_vulkan: bool,
    /// NVIDIA-GPU vorhanden? Linux: /dev/nvidia* oder libcuda.so.
    pub has_nvidia_gpu: bool,
    /// AMD-GPU vorhanden? Linux: /dev/dri + amdgpu Driver oder libamdhip64.
    pub has_amd_gpu: bool,
    /// Apple Silicon (M1/M2/...)?
    pub is_apple_silicon: bool,

    /// Welcher Build-Bundle waere optimal fuer diese Hardware?
    /// Werte: "cpu", "openblas", "vulkan", "cuda", "metal".
    pub recommended_variant: &'static str,

    /// Erwarteter Speedup-Faktor des recommended_variant gegenueber CPU-only.
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
        recommended_variant,
        recommended_speedup,
    }
}

fn pick_recommendation(
    has_openblas: bool,
    has_vulkan: bool,
    has_nvidia: bool,
    is_apple_silicon: bool,
) -> (&'static str, f32) {
    // Priorisiere: Apple Silicon > NVIDIA CUDA > Vulkan > OpenBLAS > CPU.
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
    // OpenBLAS auf Windows kommt mit dem Bundle (statisch oder mitgeliefert).
    true
}

#[cfg(target_os = "macos")]
fn detect_openblas() -> bool {
    // macOS hat Accelerate.framework — gleichwertig.
    true
}

#[cfg(target_os = "linux")]
fn detect_vulkan() -> bool {
    library_present(&["libvulkan.so.1", "libvulkan.so"])
}

#[cfg(target_os = "windows")]
fn detect_vulkan() -> bool {
    // Vulkan-Runtime kommt mit GPU-Treibern (NVIDIA, AMD, Intel).
    // Heuristik: pruefe ob vulkan-1.dll im System32 ist.
    std::path::Path::new("C:\\Windows\\System32\\vulkan-1.dll").exists()
}

#[cfg(target_os = "macos")]
fn detect_vulkan() -> bool {
    // MoltenVK gibt es zwar, aber Metal ist der bessere Weg.
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
    // Apple verkauft keine Macs mit NVIDIA-GPUs mehr.
    false
}

#[cfg(target_os = "linux")]
fn detect_amd_gpu() -> bool {
    // amdgpu-Treiber-Module geladen?
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
    // Auf Windows ist DLL-Suche komplexer; oben in detect_openblas/vulkan
    // sind die spezifischen Pfade hardcoded.
    false
}
