// SPDX-License-Identifier: GPL-3.0-or-later
//! Recording cues (short beeps on recording start/stop).
//!
//! WAVs are bundled into the binary via `include_bytes!` — no runtime
//! file IO. `rodio::OutputStream` is `!Send`, so playback runs in a
//! `spawn_blocking` task.

use crate::core::error::{Result, VoiceTypeError};
use std::io::Cursor;

const CUE_START: &[u8] = include_bytes!("../../../assets/cue_start.wav");
const CUE_STOP: &[u8] = include_bytes!("../../../assets/cue_stop.wav");

pub async fn play_start_cue() -> Result<()> {
    play_blocking(CUE_START).await
}

pub async fn play_stop_cue() -> Result<()> {
    play_blocking(CUE_STOP).await
}

async fn play_blocking(wav_bytes: &'static [u8]) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let (_stream, handle) = rodio::OutputStream::try_default()
            .map_err(|e| VoiceTypeError::Audio(format!("rodio::OutputStream: {e}")))?;
        let cursor = Cursor::new(wav_bytes);
        let source = rodio::Decoder::new(cursor)
            .map_err(|e| VoiceTypeError::Audio(format!("rodio::Decoder: {e}")))?;
        let sink = rodio::Sink::try_new(&handle)
            .map_err(|e| VoiceTypeError::Audio(format!("rodio::Sink: {e}")))?;
        sink.append(source);
        sink.sleep_until_end();
        Ok(())
    })
    .await
    .map_err(|e| VoiceTypeError::Audio(format!("Cue spawn_blocking: {e}")))?
}
