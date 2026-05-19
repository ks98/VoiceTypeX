// SPDX-License-Identifier: GPL-3.0-or-later
//! Embedded LLM via llama-cpp-2 — Phase-3b-Foundation.
//!
//! Ersetzt mittelfristig den `OllamaProcessor` als Default fuer
//! `processing = "local"`. Vorteile:
//! - Keine externe Daemon-Dependency (kein `ollama serve` mehr noetig).
//! - Modell-Lifecycle (Load/Unload, Speicher-Druck) liegt in der App,
//!   nicht im Daemon — wichtig fuer Memory-Pressure-Profile auf
//!   8-GB-Geraeten.
//! - Gemeinsamer Compute-Backend mit Whisper (Vulkan oder
//!   dynamic-backends-CUDA in Phase 3c) — eine Inferenz-Stack,
//!   konsistente Hardware-Anforderungen.
//!
//! Status: **Skeleton**. Modell-Load + Token-Generation sind
//! `todo!()`-Stubs. Task #23 ergaenzt die produktive Implementierung
//! (GGUF-Modell laden, Chat-Template, Sampling-Loop, Detokenize).
//!
//! Konzeptuelle Architektur fuer Task #23:
//! 1. `LlamaBackend::init()` einmalig (per `OnceLock`) beim ersten Call.
//! 2. `LlamaModel::load_from_file(backend, path, default_params)` —
//!    cache hinter `RwLock<Option<LlamaModel>>` wie bei `LocalTranscriber`.
//! 3. Pro `process()`-Call: neue `LlamaContext` mit ctx_params
//!    (n_ctx, n_threads), Chat-Template-Formatierung passend zum
//!    Modell (chatml fuer gemma3/llama3/qwen).
//! 4. Sampler aus `llama_cpp_2::sampler` mit temperature, top_p,
//!    repeat_penalty aus `ProcessOpts`.
//! 5. Token-Loop bis EOS oder max_tokens, Detokenize ueber
//!    `model.token_to_str`.

use crate::core::error::{Result, VoiceTypeError};
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;
use std::path::PathBuf;

/// Lokaler LLM-Processor mit eingebetteter llama.cpp-Engine.
/// **Skeleton** — Task #23 ersetzt die `todo!()`-Stubs durch produktive
/// Inferenz-Logik.
pub struct LlamaEmbeddedProcessor {
    /// Pfad zur GGUF-Modell-Datei. Wird beim ersten `process()`-Call
    /// geladen, danach gecacht. `RwLock<Option<LlamaModel>>` als
    /// Cache-Pattern (analog `LocalTranscriber.context`).
    #[allow(dead_code)] // Wird in Task #23 verwendet.
    model_path: PathBuf,
}

impl LlamaEmbeddedProcessor {
    pub fn new(model_path: PathBuf) -> Self {
        Self { model_path }
    }
}

#[async_trait]
impl Processor for LlamaEmbeddedProcessor {
    fn name(&self) -> &str {
        "llama-embedded"
    }

    async fn process(
        &self,
        _transcript: &str,
        _system_prompt: &str,
        _opts: ProcessOpts,
    ) -> Result<String> {
        // Task #23: produktive Implementierung.
        // - LlamaBackend::init (OnceLock-Singleton)
        // - LlamaModel laden, cachen
        // - Chat-Template formatieren (gemma3/llama3-spezifisch)
        // - LlamaContext + Sampler erstellen
        // - Token-Loop, Detokenize
        // - Result als String zurueckgeben
        Err(VoiceTypeError::Processing(
            "LlamaEmbeddedProcessor: produktive Inferenz noch nicht implementiert \
             (Task #23). Bitte vorerst processing = \"local\" mit Ollama-Endpoint \
             oder processing = \"cloud\" benutzen."
                .into(),
        ))
    }
}
