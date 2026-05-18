// SPDX-License-Identifier: GPL-3.0-or-later
//! Whisper- und VAD-Modell-Downloader.
//!
//! Whisper-Modelle kommen aus `ggerganov/whisper.cpp` (Hugging Face),
//! Silero-VAD aus `ggml-org/whisper-vad`. Beide Pfade nutzen die HF-Konvention
//! `resolve/main/<file>` (NICHT `blob/main`, das liefert HTML).
//!
//! SHA-256-Hashes sind pro Slot pinnbar — wo `Some(...)` hinterlegt ist,
//! laeuft eine echte Integritaetspruefung (in-flight beim Download und
//! beim Re-Use eines bereits vorhandenen Files); wo `None` steht, wird der
//! tatsaechliche Hash nur geloggt, damit man ihn nachpinnen kann.

use crate::core::error::{Result, VoiceTypeError};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const WHISPER_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";
const VAD_BASE: &str = "https://huggingface.co/ggml-org/whisper-vad/resolve/main";
// Apache-2.0, primeline-Fine-tune fuer Deutsch. cstr ist Re-Packager mit
// GGML-Konvertierung; siehe THIRD_PARTY_NOTICES.md fuer Lizenz-Kette.
const WHISPER_GERMAN_BASE: &str =
    "https://huggingface.co/cstr/whisper-large-v3-turbo-german-ggml/resolve/main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSlot {
    /// Light-Hardware-Variante (8 GB RAM, kein GPU) — kleinere Quantisierung,
    /// halber Disk-Bedarf, leicht hoeherer WER auf Deutsch.
    LargeV3TurboQ5,
    /// **Default ab Phase 1** — Q8_0 ist auf modernen Backends ~gleich schnell
    /// wie Q5_0, aber qualitativ deutlich naeher an F16. Sweet-Spot fuer
    /// 8-16 GB RAM mit iGPU/Vulkan oder dGPU.
    LargeV3TurboQ8,
    /// **DE Pro** — primeline/whisper-large-v3-turbo-german (Apache-2.0)
    /// als Q5_0-GGUF. ~28 % rel. WER-Reduktion auf deutschem
    /// CommonVoice/Tuda gegenueber Generic-Turbo. Mai 2026: nur Q5_0
    /// im cstr-Repo verfuegbar (Q8 nicht gepackt). Gleicher Disk-Bedarf
    /// wie LargeV3TurboQ5.
    LargeV3TurboGermanQ5,
    /// Spar-Fallback — kleiner, fuer 4-GB-Geraete ohne GPU.
    SmallQ51,
    /// Maximale Qualitaet (F16), groesster Disk-Bedarf. Fuer User mit
    /// ueppigem VRAM, die jedes WER-Promille wollen.
    LargeV3Turbo,
}

impl ModelSlot {
    pub fn filename(self) -> &'static str {
        match self {
            Self::LargeV3TurboQ5 => "ggml-large-v3-turbo-q5_0.bin",
            Self::LargeV3TurboQ8 => "ggml-large-v3-turbo-q8_0.bin",
            // cstr-Repo benennt das File als "ggml-model-q5_0.bin" (ohne
            // model-spezifisches Praefix). Wir behalten den Namen bei,
            // weil wir das File 1:1 von der Quelle ziehen.
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

    /// Erwarteter SHA-256, gezogen aus dem Git-LFS-Pointer im jeweiligen
    /// Hugging-Face-Repo (`curl https://huggingface.co/<repo>/raw/main/<file>
    /// | head -3` zeigt die `oid sha256:...`-Zeile). Wird sowohl in-flight
    /// beim Download als auch beim Re-Use einer bestehenden Datei geprueft.
    /// Bei Hash-Mismatch wird das File neu geladen, nicht akzeptiert.
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

    /// Mappt den persistierten Settings-String auf einen Slot. Single Source
    /// of Truth — lib.rs (Bootstrap-Pfad-Konstruktion) und ipc/settings.rs
    /// (Download-Trigger) nutzen beide diese Funktion, damit der "welcher
    /// Slot ist gerade aktiv"-Vergleich nicht zweimal divergieren kann.
    pub fn from_setting(s: &str) -> Self {
        match s {
            "small-q5_1" => Self::SmallQ51,
            "large-v3-turbo" => Self::LargeV3Turbo,
            "large-v3-turbo-q5_0" => Self::LargeV3TurboQ5,
            "large-v3-turbo-german-q5_0" => Self::LargeV3TurboGermanQ5,
            _ => Self::LargeV3TurboQ8, // Default
        }
    }

    /// HF-Repo unterscheidet sich pro Slot — Generic kommt von
    /// ggerganov/whisper.cpp, DE-Pro vom primeline/cstr-Re-Packager.
    fn url(self) -> String {
        let base = match self {
            Self::LargeV3TurboGermanQ5 => WHISPER_GERMAN_BASE,
            _ => WHISPER_BASE,
        };
        format!("{base}/{}", self.filename())
    }
}

/// Silero-VAD-Modell, das whisper.cpp's built-in VAD-Pfad braucht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadModel {
    /// v6.2.0 — aktuellste Variante (Stand Mai 2026), ~885 kB.
    /// Wird beim ersten Whisper-Modell-Download mit-gezogen, weil VAD im
    /// neuen Default-Pfad aktiviert ist (siehe local.rs).
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

/// Lade ein Whisper-Modell herunter. `dest_dir` muss existieren.
pub async fn download_model<F>(slot: ModelSlot, dest_dir: &Path, on_progress: F) -> Result<PathBuf>
where
    F: FnMut(DownloadProgress) + Send + 'static,
{
    let dest_path = dest_dir.join(slot.filename());
    download_to_file(&slot.url(), &dest_path, slot.expected_sha256(), on_progress).await
}

/// Lade das Silero-VAD-Modell herunter. Idempotent — wenn das File bereits
/// existiert (und Hash passt, falls gepinnt), passiert nichts.
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

/// Generischer File-Downloader mit in-flight SHA-256-Pruefung.
/// Nutzt eine `.partial`-Datei und renamed atomar erst nach erfolgreichem
/// Hash-Vergleich — so bleibt eine abgebrochene Aufnahme nie als
/// "ueberredet sich, das passt schon"-File liegen.
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
                tracing::info!(file = %label, "Datei bereits vorhanden + Hash OK");
                return Ok(dest_path.to_path_buf());
            }
            tracing::warn!(
                file = %label,
                "Hash mismatch — re-download (expected={expected}, got={actual})"
            );
        } else {
            tracing::info!(file = %label, "Datei vorhanden, kein Hash-Reference — akzeptiert");
            return Ok(dest_path.to_path_buf());
        }
    }

    tracing::info!(url = %url, "Download startet");

    let response = reqwest::get(url)
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("HTTP-Fehler {url}: {e}")))?;
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
                "Hash mismatch fuer {label}: expected={expected}, got={actual_hash}"
            )));
        }
    } else {
        tracing::info!(
            file = %label,
            sha256 = %actual_hash,
            "Heruntergeladen — kein Reference-Hash, ueberspringe Verifikation"
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

    /// Regression-Guard: jeder neue Slot muss einen gepinten Hash haben,
    /// sonst wird die Integritaetsverifikation beim Download stillschweigend
    /// uebersprungen. Wenn dieser Test rot ist, gehoert in `expected_sha256`
    /// der echte Hash von `huggingface.co/<repo>/raw/main/<file>`.
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
                "{slot:?} hat keinen gepinten SHA-256 — Integritaetsverifikation faellt aus"
            );
        }
    }

    #[test]
    fn all_vad_models_have_pinned_hashes() {
        for model in [VadModel::SileroV6_2_0] {
            assert!(
                model.expected_sha256().is_some(),
                "{model:?} hat keinen gepinten SHA-256"
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
}
