// SPDX-License-Identifier: GPL-3.0-or-later
//! Cloud LLM providers.
//!
//! `OpenAICompatibleClient` is the shared composition for xAI/OpenAI;
//! Anthropic is standalone (CLAUDE.md §4.6).

pub mod anthropic;
pub mod openai;
pub mod openai_compatible;
pub mod xai;
