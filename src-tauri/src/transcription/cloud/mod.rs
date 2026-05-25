// SPDX-License-Identifier: GPL-3.0-or-later
//! Cloud STT providers.
//!
//! Important (CLAUDE.md §4.6): there is **no** shared wrapper because
//! the APIs differ. xAI has its own format; OpenAI/Groq are
//! Whisper-API-compatible, but Deepgram is again distinct.

pub mod deepgram;
pub mod groq;
pub mod openai;
pub mod whisper_compatible;
pub mod xai;
