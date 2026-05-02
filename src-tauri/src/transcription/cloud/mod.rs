// SPDX-License-Identifier: GPL-3.0-or-later
//! Cloud-STT Provider.
//!
//! Wichtig (CLAUDE.md §4.6): es gibt **keinen** gemeinsamen Wrapper, weil die
//! APIs unterschiedlich sind. xAI hat eigenes Format; OpenAI/Groq sind
//! Whisper-API-kompatibel, aber Deepgram ist wieder eigen.

pub mod deepgram;
pub mod groq;
pub mod openai;
pub mod whisper_compatible;
pub mod xai;
