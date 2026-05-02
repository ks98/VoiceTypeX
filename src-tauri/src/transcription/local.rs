// SPDX-License-Identifier: GPL-3.0-or-later
//! Lokales STT via whisper.cpp (whisper-rs Bindings).
//!
//! Phase 1.2: Stub mit korrekter Trait-Signatur. Echte Integration in Phase 1.3
//! mit Lazy-Loading und Arc<Mutex<WhisperContext>>.

use crate::core::error::{Result, VoiceTypeError};
use crate::transcription::{TranscribeOpts, Transcriber, TranscriptionMode};
use async_trait::async_trait;
use std::path::PathBuf;

const SUPPORTED: &[TranscriptionMode] = &[TranscriptionMode::OneShot];

#[allow(dead_code)]
pub struct LocalTranscriber {
    model_path: PathBuf,
}

impl LocalTranscriber {
    pub fn new(model_path: PathBuf) -> Self {
        Self { model_path }
    }
}

#[async_trait]
impl Transcriber for LocalTranscriber {
    fn name(&self) -> &str {
        "local-whisper"
    }

    fn supports(&self) -> &'static [TranscriptionMode] {
        SUPPORTED
    }

    async fn transcribe_oneshot(&self, _audio: &[u8], _opts: TranscribeOpts) -> Result<String> {
        Err(VoiceTypeError::Transcription(
            "Lokales whisper-rs noch nicht eingebunden (Phase 1.3)".into(),
        ))
    }
}
