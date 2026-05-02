// SPDX-License-Identifier: GPL-3.0-or-later
//! Whisper-Modell-Downloader fuer ggerganov/whisper.cpp (Hugging Face).
//!
//! Stuetzt CLAUDE.md §2.1 — drei Modell-Slots, alle aus dem offiziellen
//! Hugging-Face-Repo. Hash-Verifikation: optional (sobald wir die SHA-256
//! der drei Modelle bestaetigt haben, fuegen wir sie als Konstanten ein).
//!
//! Die DOWNLOAD_BASE-URL nutzt `resolve/main` — das ist der offizielle
//! Download-Pfad (NICHT `blob/main`, das liefert HTML).

use crate::core::error::{Result, VoiceTypeError};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const DOWNLOAD_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSlot {
    /// Default — beste Qualitaet/Latenz-Balance fuer Diktat.
    LargeV3TurboQ5,
    /// Spar-Fallback — kleiner, etwas schwaechere deutsche Qualitaet.
    SmallQ51,
    /// Maximale Qualitaet, doppelter Disk-Bedarf gegenueber Q5.
    LargeV3Turbo,
}

impl ModelSlot {
    pub fn filename(self) -> &'static str {
        match self {
            Self::LargeV3TurboQ5 => "ggml-large-v3-turbo-q5_0.bin",
            Self::SmallQ51 => "ggml-small-q5_1.bin",
            Self::LargeV3Turbo => "ggml-large-v3-turbo.bin",
        }
    }

    pub fn approximate_size_mb(self) -> u32 {
        match self {
            Self::LargeV3TurboQ5 => 547,
            Self::SmallQ51 => 181,
            Self::LargeV3Turbo => 1_624,
        }
    }

    /// Erwarteter SHA-256, wenn bekannt. Wenn `None`, ueberspringen wir die
    /// Verifikation und protokollieren eine Warnung. Phase 1.5 ergaenzt
    /// die echten Hashes (per Settings-UI mit Hugging-Face-LFS-Etag-Lookup).
    pub fn expected_sha256(self) -> Option<&'static str> {
        match self {
            Self::LargeV3TurboQ5 => None,
            Self::SmallQ51 => None,
            Self::LargeV3Turbo => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
}

/// Lade ein Modell herunter. `dest_dir` muss existieren. Wenn die Zieldatei
/// bereits da ist und (falls Hash bekannt) der Hash passt, wird der Download
/// uebersprungen.
pub async fn download_model<F>(
    slot: ModelSlot,
    dest_dir: &Path,
    mut on_progress: F,
) -> Result<PathBuf>
where
    F: FnMut(DownloadProgress) + Send + 'static,
{
    let dest_path = dest_dir.join(slot.filename());

    if dest_path.exists() {
        if let Some(expected) = slot.expected_sha256() {
            let actual = compute_sha256(&dest_path).await?;
            if actual.eq_ignore_ascii_case(expected) {
                tracing::info!(
                    model = slot.filename(),
                    "Modell bereits vorhanden + Hash OK"
                );
                return Ok(dest_path);
            }
            tracing::warn!(
                model = slot.filename(),
                "Hash mismatch — re-download (expected={expected}, got={actual})"
            );
        } else {
            tracing::info!(
                model = slot.filename(),
                "Modell vorhanden, kein Hash-Reference — akzeptiert"
            );
            return Ok(dest_path);
        }
    }

    let url = format!("{DOWNLOAD_BASE}/{}", slot.filename());
    tracing::info!(url = %url, "Whisper-Modell-Download startet");

    let response = reqwest::get(&url)
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
    if let Some(expected) = slot.expected_sha256() {
        if !actual_hash.eq_ignore_ascii_case(expected) {
            tokio::fs::remove_file(&tmp_path).await.ok();
            return Err(VoiceTypeError::Transcription(format!(
                "Hash mismatch fuer {}: expected={expected}, got={actual_hash}",
                slot.filename()
            )));
        }
    } else {
        tracing::info!(
            model = slot.filename(),
            sha256 = %actual_hash,
            "Modell heruntergeladen — kein Reference-Hash, ueberspringe Verifikation"
        );
    }

    tokio::fs::rename(&tmp_path, &dest_path)
        .await
        .map_err(|e| VoiceTypeError::Transcription(format!("rename to final: {e}")))?;
    Ok(dest_path)
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
