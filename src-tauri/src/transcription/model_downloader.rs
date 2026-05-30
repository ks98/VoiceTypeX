// SPDX-License-Identifier: GPL-3.0-or-later
//! Whisper and VAD model downloader.
//!
//! Whisper models come from `ggerganov/whisper.cpp` (Hugging Face),
//! Silero VAD from `ggml-org/whisper-vad`. Both paths use the HF
//! convention `resolve/main/<file>` (NOT `blob/main`, which returns
//! HTML).
//!
//! SHA-256 hashes can be pinned per slot — where `Some(...)` is set, a
//! real integrity check runs (in-flight during download and when
//! re-using an already existing file); where `None` is set, the actual
//! hash is only logged so it can be pinned later.

use crate::core::error::{Result, VoiceTypeError};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const WHISPER_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const VAD_BASE: &str = "https://huggingface.co/ggml-org/whisper-vad/resolve/main";
// Apache-2.0, primeline fine-tune for German. cstr is the re-packager
// with GGML conversion; see THIRD_PARTY_NOTICES.md for the license chain.
const WHISPER_GERMAN_BASE: &str =
    "https://huggingface.co/cstr/whisper-large-v3-turbo-german-ggml/resolve/main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSlot {
    /// Light-hardware variant (8 GB RAM, no GPU) — smaller quantization,
    /// half the disk footprint, slightly higher WER on German.
    LargeV3TurboQ5,
    /// **Default since phase 1** — Q8_0 is roughly as fast as Q5_0 on
    /// modern backends, but qualitatively much closer to F16. Sweet-spot
    /// for 8-16 GB RAM with iGPU/Vulkan or dGPU.
    LargeV3TurboQ8,
    /// **DE Pro** — primeline/whisper-large-v3-turbo-german (Apache-2.0)
    /// as Q5_0 GGUF. ~28 % rel. WER reduction on German
    /// CommonVoice/Tuda over the generic turbo. May 2026: only Q5_0
    /// available in the cstr repo (Q8 not packed). Same disk footprint
    /// as `LargeV3TurboQ5`.
    LargeV3TurboGermanQ5,
    /// Frugal fallback — smaller, for 4 GB devices without GPU.
    SmallQ51,
    /// Maximum quality (F16), largest disk footprint. For users with
    /// abundant VRAM who want every WER permille.
    LargeV3Turbo,
}

impl ModelSlot {
    pub fn filename(self) -> &'static str {
        match self {
            Self::LargeV3TurboQ5 => "ggml-large-v3-turbo-q5_0.bin",
            Self::LargeV3TurboQ8 => "ggml-large-v3-turbo-q8_0.bin",
            // The cstr repo names the file "ggml-model-q5_0.bin" (without
            // a model-specific prefix). We keep the name because we pull
            // the file 1:1 from the source.
            Self::LargeV3TurboGermanQ5 => "ggml-model-q5_0.bin",
            Self::SmallQ51 => "ggml-small-q5_1.bin",
            Self::LargeV3Turbo => "ggml-large-v3-turbo.bin",
        }
    }

    pub fn approximate_size_mb(self) -> u32 {
        match self {
            Self::LargeV3TurboQ5 => 547,
            Self::LargeV3TurboQ8 => 874,
            Self::LargeV3TurboGermanQ5 => 574,
            Self::SmallQ51 => 181,
            Self::LargeV3Turbo => 1_624,
        }
    }

    /// Expected SHA-256, pulled from the Git-LFS pointer in the
    /// respective Hugging Face repo (`curl
    /// https://huggingface.co/<repo>/raw/main/<file> | head -3` shows
    /// the `oid sha256:...` line). Checked both in-flight during
    /// download and on re-use of an existing file. On hash mismatch the
    /// file is re-downloaded, not accepted.
    pub fn expected_sha256(self) -> Option<&'static str> {
        match self {
            Self::LargeV3TurboQ5 => {
                Some("394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2")
            }
            Self::LargeV3TurboQ8 => {
                Some("317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1")
            }
            Self::LargeV3TurboGermanQ5 => {
                Some("15e92e3db0993c52fffa781513eec9253475331c1be808f8fb409285c9d9d030")
            }
            Self::SmallQ51 => {
                Some("ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb")
            }
            Self::LargeV3Turbo => {
                Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69")
            }
        }
    }

    /// Maps the persisted settings string to a slot. Single source of
    /// truth — lib.rs (bootstrap path construction) and ipc/settings.rs
    /// (download trigger) both use this function so the "which slot is
    /// currently active" comparison cannot diverge in two places.
    pub fn from_setting(s: &str) -> Self {
        match s {
            "small-q5_1" => Self::SmallQ51,
            "large-v3-turbo" => Self::LargeV3Turbo,
            "large-v3-turbo-q5_0" => Self::LargeV3TurboQ5,
            "large-v3-turbo-german-q5_0" => Self::LargeV3TurboGermanQ5,
            _ => Self::LargeV3TurboQ8, // Default
        }
    }

    /// The HF repo differs per slot — generic comes from
    /// ggerganov/whisper.cpp, DE-Pro from the primeline/cstr re-packager.
    fn url(self) -> String {
        let base = match self {
            Self::LargeV3TurboGermanQ5 => WHISPER_GERMAN_BASE,
            _ => WHISPER_BASE,
        };
        format!("{base}/{}", self.filename())
    }
}

// GGUF sources for the embedded LLM path (phase 3b + refresh May 2026).
// Prefer `unsloth/*` because its GGUF re-packs are publicly accessible
// (unlike `bartowski/gemma-*` and the original `google/*`, which have a
// license gate).
const LLM_UNSLOTH_GEMMA4_E4B: &str =
    "https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF/resolve/main";
const LLM_UNSLOTH_GEMMA4_E2B: &str =
    "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/main";
const LLM_UNSLOTH_GEMMA3_4B: &str =
    "https://huggingface.co/unsloth/gemma-3-4b-it-GGUF/resolve/main";
const LLM_UNSLOTH_GEMMA3_1B: &str =
    "https://huggingface.co/unsloth/gemma-3-1b-it-GGUF/resolve/main";
const LLM_UNSLOTH_LLAMA32_1B: &str =
    "https://huggingface.co/unsloth/Llama-3.2-1B-Instruct-GGUF/resolve/main";
const LLM_QWEN25_15B: &str = "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main";

/// GGUF LLM models that the `LlamaEmbeddedProcessor` can load.
/// Selection tiers (May 2026):
/// - **Light** (4-8 GB RAM): Gemma3-1B or Llama3.2-1B (<1 GB disk,
///   ~1.5 GB RAM at 4-bit). Gemma 4 does not fit here — the matformer
///   architecture has more raw params and needs ~3 GB disk even in the
///   smallest E2B format.
/// - **Mid** (8-12 GB): Qwen2.5-1.5B or **Gemma 4 E2B** (new, ~3 GB
///   disk, ~5 GB RAM 4-bit) — sweet-spot for 8 GB notebooks.
/// - **Pro** (12+ GB): **Gemma 4 E4B** (new, ~5 GB disk, ~5-7 GB RAM
///   4-bit) — best DE quality, multimodal-capable (we only use text).
///   Replaces Gemma 3 4B as the pro default.
/// - Gemma 3 4B remains as a backward-compat option for users who don't
///   want to give up the smaller disk size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmModelSlot {
    /// **Pro-tier default since May 2026** — Gemma 4 E4B-IT Q5_K_M.
    /// Apache 2.0, 4.5 B effective params, 140+ languages, 256k context,
    /// llama.cpp support since April 2026. ~5.1 GB disk, ~6 GB RAM at
    /// 4-bit inference.
    Gemma4E4bItQ5km,
    /// **Mid-tier default since May 2026** — Gemma 4 E2B-IT Q5_K_M.
    /// Apache 2.0, 2.3 B effective params. ~3.1 GB disk, ~5 GB RAM
    /// 4-bit. Sweet-spot for 8-12 GB setups.
    Gemma4E2bItQ5km,
    /// Gemma 3 4B-IT Q5_K_M — phase-1 recommendation, now a
    /// backward-compat variant. 140+ languages, very strong on German.
    /// ~2.8 GB.
    Gemma3_4bItQ5km,
    /// Gemma 3 1B-IT Q5_K_M — **light-tier default** (851 MB), fits on
    /// 4 GB VMs. Gemma 4 is too big here.
    Gemma3_1bItQ5km,
    /// Llama 3.2 1B-Instruct Q5_K_M — alternative in the light tier,
    /// stronger on English, but quite decent on German.
    Llama32_1bInstructQ5km,
    /// Qwen 2.5 1.5B-Instruct Q5_K_M — mid-size (~1.3 GB), strong on
    /// structured output / code (the "claude_code_anweisung" mode would
    /// have benefited from this if it ran locally).
    Qwen25_15bInstructQ5km,
}

impl LlmModelSlot {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Gemma4E4bItQ5km => "gemma-4-E4B-it-Q5_K_M.gguf",
            Self::Gemma4E2bItQ5km => "gemma-4-E2B-it-Q5_K_M.gguf",
            Self::Gemma3_4bItQ5km => "gemma-3-4b-it-Q5_K_M.gguf",
            Self::Gemma3_1bItQ5km => "gemma-3-1b-it-Q5_K_M.gguf",
            Self::Llama32_1bInstructQ5km => "Llama-3.2-1B-Instruct-Q5_K_M.gguf",
            Self::Qwen25_15bInstructQ5km => "qwen2.5-1.5b-instruct-q5_k_m.gguf",
        }
    }

    pub fn approximate_size_mb(self) -> u32 {
        match self {
            Self::Gemma4E4bItQ5km => 5_482,
            Self::Gemma4E2bItQ5km => 3_356,
            Self::Gemma3_4bItQ5km => 2_829,
            Self::Gemma3_1bItQ5km => 851,
            Self::Llama32_1bInstructQ5km => 912,
            Self::Qwen25_15bInstructQ5km => 1_285,
        }
    }

    /// SHA-256 from the HF Git-LFS pointer
    /// (`curl https://huggingface.co/<repo>/raw/main/<file> | head -3`).
    pub fn expected_sha256(self) -> Option<&'static str> {
        match self {
            Self::Gemma4E4bItQ5km => {
                Some("49bfb8a0cf4a35b74acd30bd1c9867061ccd4bd25336834e46bc608641ec8111")
            }
            Self::Gemma4E2bItQ5km => {
                Some("d8fc2ac6fd597481dfd9c5ef9543ea1f0bda8088086da3853ce5e5564ab43bf8")
            }
            Self::Gemma3_4bItQ5km => {
                Some("974e5c2f13c321fc3258b6fbf2ce326a09d8ace511aa6846df1db62baf7df7d4")
            }
            Self::Gemma3_1bItQ5km => {
                Some("0da75a587ce0be8ea0281d5c6453822c3c347ce524b6cc14b129fb137caa8a6a")
            }
            Self::Llama32_1bInstructQ5km => {
                Some("69dce91345442121eb3195370337eefa02cf076c7d84bd39adc0ce9552ccdfef")
            }
            Self::Qwen25_15bInstructQ5km => {
                Some("b46661073c18e5b56a41fa320975f866a00def1ff08feef4718e013258896f8c")
            }
        }
    }

    /// Maps the persisted settings string to a slot. On unknown values
    /// it falls back to the smallest model — a safer default for
    /// memory-constrained devices.
    pub fn from_setting(s: &str) -> Self {
        match s {
            "gemma4-e4b-it-q5_k_m" => Self::Gemma4E4bItQ5km,
            "gemma4-e2b-it-q5_k_m" => Self::Gemma4E2bItQ5km,
            "gemma3-4b-it-q5_k_m" => Self::Gemma3_4bItQ5km,
            "llama3.2-1b-instruct-q5_k_m" => Self::Llama32_1bInstructQ5km,
            "qwen2.5-1.5b-instruct-q5_k_m" => Self::Qwen25_15bInstructQ5km,
            _ => Self::Gemma3_1bItQ5km,
        }
    }

    fn url(self) -> String {
        let base = match self {
            Self::Gemma4E4bItQ5km => LLM_UNSLOTH_GEMMA4_E4B,
            Self::Gemma4E2bItQ5km => LLM_UNSLOTH_GEMMA4_E2B,
            Self::Gemma3_4bItQ5km => LLM_UNSLOTH_GEMMA3_4B,
            Self::Gemma3_1bItQ5km => LLM_UNSLOTH_GEMMA3_1B,
            Self::Llama32_1bInstructQ5km => LLM_UNSLOTH_LLAMA32_1B,
            Self::Qwen25_15bInstructQ5km => LLM_QWEN25_15B,
        };
        format!("{base}/{}", self.filename())
    }
}

/// Download a GGUF LLM model. Reuses the generic `download_to_file`
/// helper. Idempotent — no re-download if the file already exists with
/// the matching hash.
pub async fn download_llm<F>(slot: LlmModelSlot, dest_dir: &Path, on_progress: F) -> Result<PathBuf>
where
    F: FnMut(DownloadProgress) + Send + 'static,
{
    let dest_path = dest_dir.join(slot.filename());
    download_to_file(&slot.url(), &dest_path, slot.expected_sha256(), on_progress).await
}

/// Silero-VAD model that whisper.cpp's built-in VAD path requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadModel {
    /// v6.2.0 — latest variant (as of May 2026), ~885 kB. Pulled
    /// alongside the first Whisper model download, because VAD is
    /// enabled in the new default path (see local.rs).
    SileroV6_2_0,
}

impl VadModel {
    pub fn filename(self) -> &'static str {
        match self {
            Self::SileroV6_2_0 => "ggml-silero-v6.2.0.bin",
        }
    }

    pub fn approximate_size_kb(self) -> u32 {
        match self {
            Self::SileroV6_2_0 => 885,
        }
    }

    pub fn expected_sha256(self) -> Option<&'static str> {
        match self {
            Self::SileroV6_2_0 => {
                Some("2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987")
            }
        }
    }

    fn url(self) -> String {
        format!("{VAD_BASE}/{}", self.filename())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
}

/// Download a Whisper model. `dest_dir` must exist.
pub async fn download_model<F>(slot: ModelSlot, dest_dir: &Path, on_progress: F) -> Result<PathBuf>
where
    F: FnMut(DownloadProgress) + Send + 'static,
{
    let dest_path = dest_dir.join(slot.filename());
    download_to_file(&slot.url(), &dest_path, slot.expected_sha256(), on_progress).await
}

/// Download the Silero VAD model. Idempotent — nothing happens if the
/// file already exists (and the hash matches, when pinned).
pub async fn download_vad<F>(model: VadModel, dest_dir: &Path, on_progress: F) -> Result<PathBuf>
where
    F: FnMut(DownloadProgress) + Send + 'static,
{
    let dest_path = dest_dir.join(model.filename());
    download_to_file(
        &model.url(),
        &dest_path,
        model.expected_sha256(),
        on_progress,
    )
    .await
}

/// Generic file downloader with in-flight SHA-256 verification. Uses a
/// `.partial` file and renames atomically only after a successful hash
/// comparison — so an aborted download never leaves behind a
/// "convinced itself it's fine" file.
async fn download_to_file<F>(
    url: &str,
    dest_path: &Path,
    expected_sha256: Option<&str>,
    mut on_progress: F,
) -> Result<PathBuf>
where
    F: FnMut(DownloadProgress) + Send + 'static,
{
    let label = dest_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unbekannt>".into());

    if dest_path.exists() {
        if let Some(expected) = expected_sha256 {
            let actual = compute_sha256(dest_path).await?;
            if actual.eq_ignore_ascii_case(expected) {
                tracing::info!(file = %label, "File already present + hash OK");
                return Ok(dest_path.to_path_buf());
            }
            tracing::warn!(
                file = %label,
                "Hash mismatch — re-download (expected={expected}, got={actual})"
            );
        } else {
            tracing::info!(file = %label, "File present, no hash reference — accepted");
            return Ok(dest_path.to_path_buf());
        }
    }

    tracing::info!(url = %url, "Download startet");

    let response = reqwest::get(url)
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("HTTP error {url}: {e}")))?;
    if !response.status().is_success() {
        return Err(VoiceTypeError::Transcription(format!(
            "Download HTTP-Status {}: {}",
            response.status(),
            url
        )));
    }
    let total = response.content_length();

    let tmp_path = dest_path.with_extension("partial");
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("create {tmp_path:?}: {e}")))?;

    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| VoiceTypeError::Transcription(format!("stream: {e}")))?;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| VoiceTypeError::Transcription(format!("write: {e}")))?;
        downloaded += chunk.len() as u64;
        on_progress(DownloadProgress {
            bytes_downloaded: downloaded,
            bytes_total: total,
        });
    }
    file.flush()
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("flush: {e}")))?;
    drop(file);

    let actual_hash = format!("{:x}", hasher.finalize());
    if let Some(expected) = expected_sha256 {
        if !actual_hash.eq_ignore_ascii_case(expected) {
            tokio::fs::remove_file(&tmp_path).await.ok();
            return Err(VoiceTypeError::Transcription(format!(
                "Hash mismatch for {label}: expected={expected}, got={actual_hash}"
            )));
        }
    } else {
        tracing::info!(
            file = %label,
            sha256 = %actual_hash,
            "Downloaded — no reference hash, skipping verification"
        );
    }

    tokio::fs::rename(&tmp_path, dest_path)
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("rename to final: {e}")))?;
    Ok(dest_path.to_path_buf())
}

async fn compute_sha256(path: &Path) -> Result<String> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("open {path:?}: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| VoiceTypeError::Transcription(format!("read: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard: every new slot must have a pinned hash,
    /// otherwise the integrity verification during download is silently
    /// skipped. If this test is red, the real hash from
    /// `huggingface.co/<repo>/raw/main/<file>` belongs in
    /// `expected_sha256`.
    #[test]
    fn all_whisper_slots_have_pinned_hashes() {
        for slot in [
            ModelSlot::LargeV3TurboQ5,
            ModelSlot::LargeV3TurboQ8,
            ModelSlot::LargeV3TurboGermanQ5,
            ModelSlot::SmallQ51,
            ModelSlot::LargeV3Turbo,
        ] {
            assert!(
                slot.expected_sha256().is_some(),
                "{slot:?} has no pinned SHA-256 — integrity verification disabled"
            );
        }
    }

    #[test]
    #[allow(clippy::single_element_loop)]
    fn all_vad_models_have_pinned_hashes() {
        for model in [VadModel::SileroV6_2_0] {
            assert!(
                model.expected_sha256().is_some(),
                "{model:?} has no pinned SHA-256"
            );
        }
    }

    #[test]
    fn from_setting_default_is_q8() {
        assert_eq!(
            ModelSlot::from_setting("unbekannt"),
            ModelSlot::LargeV3TurboQ8
        );
        assert_eq!(ModelSlot::from_setting(""), ModelSlot::LargeV3TurboQ8);
    }

    #[test]
    fn all_llm_slots_have_pinned_hashes() {
        for slot in [
            LlmModelSlot::Gemma4E4bItQ5km,
            LlmModelSlot::Gemma4E2bItQ5km,
            LlmModelSlot::Gemma3_4bItQ5km,
            LlmModelSlot::Gemma3_1bItQ5km,
            LlmModelSlot::Llama32_1bInstructQ5km,
            LlmModelSlot::Qwen25_15bInstructQ5km,
        ] {
            assert!(
                slot.expected_sha256().is_some(),
                "{slot:?} has no pinned SHA-256"
            );
        }
    }

    #[test]
    fn llm_from_setting_recognizes_new_gemma4_slugs() {
        assert_eq!(
            LlmModelSlot::from_setting("gemma4-e4b-it-q5_k_m"),
            LlmModelSlot::Gemma4E4bItQ5km
        );
        assert_eq!(
            LlmModelSlot::from_setting("gemma4-e2b-it-q5_k_m"),
            LlmModelSlot::Gemma4E2bItQ5km
        );
        // Backward-compat: Gemma 3 slugs remain recognizable
        assert_eq!(
            LlmModelSlot::from_setting("gemma3-4b-it-q5_k_m"),
            LlmModelSlot::Gemma3_4bItQ5km
        );
        // Default fallback on unknown values
        assert_eq!(
            LlmModelSlot::from_setting("nonexistent"),
            LlmModelSlot::Gemma3_1bItQ5km
        );
    }

    #[test]
    fn from_setting_recognizes_all_known_slugs() {
        assert_eq!(
            ModelSlot::from_setting("large-v3-turbo-q8_0"),
            ModelSlot::LargeV3TurboQ8
        );
        assert_eq!(
            ModelSlot::from_setting("large-v3-turbo-q5_0"),
            ModelSlot::LargeV3TurboQ5
        );
        assert_eq!(
            ModelSlot::from_setting("large-v3-turbo-german-q5_0"),
            ModelSlot::LargeV3TurboGermanQ5
        );
        assert_eq!(ModelSlot::from_setting("small-q5_1"), ModelSlot::SmallQ51);
        assert_eq!(
            ModelSlot::from_setting("large-v3-turbo"),
            ModelSlot::LargeV3Turbo
        );
    }

    /// Integration test against Hugging Face: pulls the real Silero-VAD
    /// file (~885 kB, smallest pinned file) and verifies its SHA-256
    /// against the hash pinned in the code. Fails if the HF repo
    /// swaps out the file — in which case the real download path falls
    /// apart live, without the structure tests noticing.
    ///
    /// `#[ignore]` because CI containers and sandbox worktrees often
    /// have no network access. Run locally with:
    ///     cargo test --lib silero_vad -- --ignored
    /// In the release job a dedicated step should run this test with
    /// the `--ignored` flag so a repo drift surfaces before the tag.
    #[tokio::test]
    #[ignore = "needs network access to huggingface.co"]
    async fn silero_vad_real_download_hash_matches_pinned() {
        let dest_dir =
            std::env::temp_dir().join(format!("voicetypex-vad-hash-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&dest_dir)
            .await
            .expect("create temp dir");

        // If a previous test run already left the file here: drop it,
        // otherwise `download_to_file` takes the existing hash-match
        // path and no longer exercises the wire protocol.
        let expected_file = dest_dir.join(VadModel::SileroV6_2_0.filename());
        if expected_file.exists() {
            tokio::fs::remove_file(&expected_file)
                .await
                .expect("cleanup pre-existing test file");
        }

        let result = download_vad(VadModel::SileroV6_2_0, &dest_dir, |_| {}).await;

        // Cleanup before the assert, so no test file lingers in /tmp
        // on failure.
        let _ = tokio::fs::remove_dir_all(&dest_dir).await;

        let path = result.expect("Silero-VAD download must succeed");
        assert!(path.ends_with(VadModel::SileroV6_2_0.filename()));
    }
}
