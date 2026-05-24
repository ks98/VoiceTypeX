// SPDX-License-Identifier: GPL-3.0-or-later
//! Embedded LLM via llama-cpp-2 — Phase 3b.
//!
//! Ersetzt den `OllamaProcessor` als Default fuer `processing = "local"`.
//! Vorteile:
//! - Keine externe Daemon-Dependency (kein `ollama serve` mehr noetig).
//! - Modell-Lifecycle (Load/Unload, Speicher-Druck) liegt in der App,
//!   nicht im Daemon — wichtig fuer Memory-Pressure-Profile auf
//!   8-GB-Geraeten.
//! - Gemeinsamer Compute-Backend mit Whisper (Vulkan oder
//!   dynamic-backends-CUDA in einer Folge-Iteration) — eine Inferenz-
//!   Stack, konsistente Hardware-Anforderungen.
//!
//! Pipeline pro `process()`-Aufruf:
//! 1. `LlamaBackend::init()` via `OnceLock` — einmaliger Singleton-
//!    Setup beim ersten Call.
//! 2. `LlamaModel::load_from_file` — gecacht hinter `RwLock<Option<_>>`,
//!    analog zu `LocalTranscriber`.
//! 3. Chat-Template aus dem GGUF-Modell ziehen (`model.chat_template
//!    (None)`), Messages formatieren.
//! 4. `model.str_to_token` → Vec<LlamaToken>.
//! 5. Frischer `LlamaContext` + `LlamaBatch` pro Inferenz.
//! 6. `LlamaSampler::chain_simple` mit penalties + top_p + temp + dist.
//! 7. Decode-Loop bis EOG-Token oder `max_tokens`.

// `Special::Plaintext` und `token_to_str` sind in llama-cpp-2 0.1.146
// als deprecated markiert (zukuenftig → `token_to_piece`). Fuer Phase 3b
// nutzen wir die einfachere API; Migration auf `token_to_piece` ist
// Polish-Task. Bis dahin allow(deprecated) auf Modul-Ebene.
#![allow(deprecated)]

use crate::core::error::{Result, VoiceTypeError};
use crate::processing::{ProcessOpts, Processor};
use async_trait::async_trait;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// Backend-Singleton. `LlamaBackend::init()` darf nur einmal pro Prozess
/// laufen — sonst gibt es `BackendAlreadyInitialized`. `OnceLock`
/// garantiert Thread-Safety und idempotenten Init.
static LLAMA_BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

fn ensure_backend() -> Result<&'static LlamaBackend> {
    if let Some(b) = LLAMA_BACKEND.get() {
        return Ok(b);
    }
    let backend = LlamaBackend::init()
        .map_err(|e| VoiceTypeError::Processing(format!("LlamaBackend::init failed: {e}")))?;
    // Race ist OK: get_or_init verliert, der erste setzt den Wert.
    Ok(LLAMA_BACKEND.get_or_init(|| backend))
}

pub struct LlamaEmbeddedProcessor {
    model_path: PathBuf,
    /// Modell-Cache — geladen beim ersten `process()`-Aufruf, gehalten
    /// fuer die Lebenszeit des Processors. Read-Lock ueber den ganzen
    /// Inferenz-Block, weil `LlamaContext` per `<'a>` an die Model-
    /// Referenz gebunden ist.
    model: Arc<RwLock<Option<LlamaModel>>>,
}

impl LlamaEmbeddedProcessor {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            model: Arc::new(RwLock::new(None)),
        }
    }

    /// Modell laden, wenn noch nicht geschehen. Idempotent.
    fn ensure_loaded(&self) -> Result<()> {
        if self.model.read().is_some() {
            return Ok(());
        }
        let mut guard = self.model.write();
        if guard.is_some() {
            return Ok(());
        }
        if !self.model_path.exists() {
            return Err(VoiceTypeError::Processing(format!(
                "LLM-Modell-Datei fehlt: {} (bitte ueber Settings herunterladen)",
                self.model_path.display()
            )));
        }
        let backend = ensure_backend()?;
        let params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(backend, &self.model_path, &params)
            .map_err(|e| VoiceTypeError::Processing(format!("LlamaModel::load_from_file: {e}")))?;
        *guard = Some(model);
        tracing::info!(model = %self.model_path.display(), "LLM model loaded");
        Ok(())
    }
}

#[async_trait]
impl Processor for LlamaEmbeddedProcessor {
    fn name(&self) -> &str {
        "llama-embedded"
    }

    async fn process(
        &self,
        transcript: &str,
        system_prompt: &str,
        opts: ProcessOpts,
    ) -> Result<String> {
        self.ensure_loaded()?;

        let model_arc = Arc::clone(&self.model);
        let transcript = transcript.to_string();
        let system_prompt = system_prompt.to_string();
        let temperature = opts.temperature.unwrap_or(0.2);
        let top_p = opts.top_p.unwrap_or(0.8);
        let repeat_penalty = opts.repeat_penalty.unwrap_or(1.05);
        let max_tokens = opts.max_tokens.unwrap_or(1024) as i32;

        tokio::task::spawn_blocking(move || -> Result<String> {
            run_llama_blocking(
                &model_arc,
                transcript,
                system_prompt,
                temperature,
                top_p,
                repeat_penalty,
                max_tokens,
            )
        })
        .await
        .map_err(|e| VoiceTypeError::Processing(format!("spawn_blocking: {e}")))?
    }
}

/// 7 Argumente sind hier vertretbar, weil sie verschiedene Concerns
/// kapseln (Modell, Prompt-Teile, Sampling). Eine Konfig-Struct waere
/// nur "7 Argumente in einer anderen Verpackung".
#[allow(clippy::too_many_arguments)]
fn run_llama_blocking(
    model_arc: &Arc<RwLock<Option<LlamaModel>>>,
    transcript: String,
    system_prompt: String,
    temperature: f32,
    top_p: f32,
    repeat_penalty: f32,
    max_tokens: i32,
) -> Result<String> {
    let guard = model_arc.read();
    let model = guard
        .as_ref()
        .ok_or_else(|| VoiceTypeError::Processing("LLM model not loaded".into()))?;
    let backend = LLAMA_BACKEND
        .get()
        .ok_or_else(|| VoiceTypeError::Processing("LlamaBackend not initialised".into()))?;

    // Chat-Template aus dem GGUF holen. Fast alle modernen Modelle
    // (gemma3, llama3, qwen3, mistral-nemo) haben ein passendes
    // bundled. None = nutze den vom Modell gespeicherten Default.
    let template = model
        .chat_template(None)
        .map_err(|e| VoiceTypeError::Processing(format!("chat_template: {e}")))?;

    let messages = vec![
        LlamaChatMessage::new("system".to_string(), system_prompt)
            .map_err(|e| VoiceTypeError::Processing(format!("LlamaChatMessage system: {e}")))?,
        LlamaChatMessage::new("user".to_string(), transcript)
            .map_err(|e| VoiceTypeError::Processing(format!("LlamaChatMessage user: {e}")))?,
    ];
    // add_ass=true: Template endet mit dem assistant-Opening-Tag, das
    // Modell muss nicht selbst einen "assistant:"-Header generieren.
    let prompt = model
        .apply_chat_template(&template, &messages, true)
        .map_err(|e| VoiceTypeError::Processing(format!("apply_chat_template: {e}")))?;

    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| VoiceTypeError::Processing(format!("str_to_token: {e}")))?;

    let prompt_len = tokens.len() as i32;
    if prompt_len > 4000 {
        // Default-ctx 4096; ueber 4000 Prompt-Tokens haben wir <100
        // Tokens fuer die Antwort. Diktat-Use-Case sollte das nie
        // erreichen, aber defensiv pruefen.
        return Err(VoiceTypeError::Processing(format!(
            "Prompt zu lang ({prompt_len} Tokens) — Diktat verkuerzen oder ctx_size erhoehen"
        )));
    }

    let ctx_params = LlamaContextParams::default();
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| VoiceTypeError::Processing(format!("new_context: {e}")))?;

    // Batch fuer den Prompt aufbauen. Logits nur fuer das letzte Token,
    // weil wir nur die naechste Token-Vorhersage brauchen.
    let mut batch = LlamaBatch::new(prompt_len as usize + max_tokens as usize, 1);
    let last_idx = prompt_len - 1;
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i as i32 == last_idx;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| VoiceTypeError::Processing(format!("batch.add prompt: {e}")))?;
    }
    ctx.decode(&mut batch)
        .map_err(|e| VoiceTypeError::Processing(format!("decode prompt: {e}")))?;

    // Sampler-Kette: penalties → top_p → temperature → dist.
    // Bei temperature == 0 nutzen wir greedy statt dist, fuer
    // strict deterministische Outputs (Rewriting-Use-Case).
    let mut sampler = if temperature <= 0.0 {
        LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, repeat_penalty, 0.0, 0.0),
            LlamaSampler::greedy(),
        ])
    } else {
        LlamaSampler::chain_simple([
            LlamaSampler::penalties(64, repeat_penalty, 0.0, 0.0),
            LlamaSampler::top_p(top_p, 1),
            LlamaSampler::temp(temperature),
            // Deterministischer Seed (0): mit temp>0 immer noch
            // stochastisch *innerhalb* der reduzierten Distribution,
            // aber zwischen App-Starts reproduzierbar.
            LlamaSampler::dist(0),
        ])
    };

    let mut output = String::new();
    let mut cursor = prompt_len;
    for _ in 0..max_tokens {
        // -1 = letztes Token im Batch (das mit logits=true gesetzt war).
        let token = sampler.sample(&ctx, -1);
        if model.is_eog_token(token) {
            break;
        }
        // Special::Plaintext = Sonder-Tokens (chat-Tags etc.) werden
        // weggelassen, nur "sichtbarer" Text kommt raus.
        let piece = model
            .token_to_str(token, Special::Plaintext)
            .map_err(|e| VoiceTypeError::Processing(format!("token_to_str: {e}")))?;
        output.push_str(&piece);

        // Naechstes Decode mit dem gesamplten Token fuettern.
        batch.clear();
        batch
            .add(token, cursor, &[0], true)
            .map_err(|e| VoiceTypeError::Processing(format!("batch.add gen: {e}")))?;
        cursor += 1;
        ctx.decode(&mut batch)
            .map_err(|e| VoiceTypeError::Processing(format!("decode gen: {e}")))?;
    }

    Ok(output.trim().to_string())
}
