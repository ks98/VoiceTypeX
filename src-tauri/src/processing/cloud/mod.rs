// SPDX-License-Identifier: GPL-3.0-or-later
//! Cloud-LLM Provider.
//!
//! `OpenAICompatibleClient` ist die geteilte Komposition fuer xAI/OpenAI;
//! Anthropic ist eigenstaendig (CLAUDE.md §4.6).

pub mod anthropic;
pub mod openai;
pub mod openai_compatible;
pub mod xai;
