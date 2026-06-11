// SPDX-License-Identifier: GPL-3.0-or-later
//! Embedded LLM via llama-cpp-2 — phase 3b.
//!
//! Replaces the `OllamaProcessor` as the default for
//! `processing = "local"`. Advantages:
//! - No external daemon dependency (no more `ollama serve` needed).
//! - The model lifecycle (load/unload, memory pressure) sits in the
//!   app, not in the daemon — important for memory-pressure profiles
//!   on 8 GB devices.
//! - Shared compute backend with Whisper (Vulkan, or dynamic-backends
//!   CUDA in a follow-up iteration) — one inference stack, consistent
//!   hardware requirements.
//!
//! Pipeline per `process()` call:
//! 1. `LlamaBackend::init()` via `OnceLock` — one-time singleton setup
//!    on the first call.
//! 2. `LlamaModel::load_from_file` — cached behind
//!    `RwLock<Option<_>>`, analogous to `LocalTranscriber`.
//! 3. Pull the chat template from the GGUF model
//!    (`model.chat_template(None)`), format messages.
//! 4. `model.str_to_token` → `Vec<LlamaToken>`.
//! 5. A fresh `LlamaContext` + `LlamaBatch` per inference.
//! 6. `LlamaSampler::chain_simple` with penalties + top_p + temp +
//!    dist.
//! 7. Decode loop until the EOG token or `max_tokens`.

// `Special::Plaintext` and `token_to_str` are marked deprecated in
// llama-cpp-2 0.1.146 (going forward: `token_to_piece`). For phase 3b
// we use the simpler API; migration to `token_to_piece` is a polish
// task. Until then `allow(deprecated)` at module level.
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
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Backend singleton. `LlamaBackend::init()` may only run once per
/// process — otherwise it returns `BackendAlreadyInitialized`.
/// `OnceLock` guarantees thread safety and idempotent init.
static LLAMA_BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

fn ensure_backend() -> Result<&'static LlamaBackend> {
    if let Some(b) = LLAMA_BACKEND.get() {
        return Ok(b);
    }
    let backend = LlamaBackend::init()
        .map_err(|e| VoiceTypeError::processing(format!("LlamaBackend::init failed: {e}")))?;
    // Races are fine: `get_or_init` drops the loser, the first one
    // sets the value.
    Ok(LLAMA_BACKEND.get_or_init(|| backend))
}

pub struct LlamaEmbeddedProcessor {
    model_path: PathBuf,
    /// Model cache — loaded on the first `process()` call, held for
    /// the processor's lifetime. Read lock over the entire inference
    /// block, because `LlamaContext` is bound to the model reference
    /// via `<'a>`.
    model: Arc<RwLock<Option<LlamaModel>>>,
}

impl LlamaEmbeddedProcessor {
    /// Constructs a processor pointing at `model_path`. The model is
    /// not opened here — the first `process()` call triggers the lazy
    /// load via `ensure_loaded`.
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            model: Arc::new(RwLock::new(None)),
        }
    }
}

/// Lazily loads the GGUF model into the llama-cpp-2 context. Subsequent
/// calls are no-ops thanks to the inner double-checked lock —
/// idempotent. Runs on the blocking pool (called inside
/// `spawn_blocking`), so the ~5 GB load and the write lock never sit on
/// an async runtime thread.
fn ensure_loaded(model: &Arc<RwLock<Option<LlamaModel>>>, model_path: &Path) -> Result<()> {
    if model.read().is_some() {
        return Ok(());
    }
    let mut guard = model.write();
    if guard.is_some() {
        return Ok(());
    }
    if !model_path.exists() {
        return Err(VoiceTypeError::processing(format!(
            "LLM model file missing: {} (download it via Settings)",
            model_path.display()
        )));
    }
    let backend = ensure_backend()?;
    let params = LlamaModelParams::default();
    let loaded = LlamaModel::load_from_file(backend, model_path, &params)
        .map_err(|e| VoiceTypeError::processing(format!("LlamaModel::load_from_file: {e}")))?;
    *guard = Some(loaded);
    tracing::info!(model = %model_path.display(), "LLM model loaded");
    Ok(())
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
        let model_arc = Arc::clone(&self.model);
        let model_path = self.model_path.clone();
        let transcript = transcript.to_string();
        let system_prompt = system_prompt.to_string();
        let temperature = opts.temperature.unwrap_or(0.2);
        let top_p = opts.top_p.unwrap_or(0.8);
        let repeat_penalty = opts.repeat_penalty.unwrap_or(1.05);
        let max_tokens = opts.max_tokens.unwrap_or(1024) as i32;

        tokio::task::spawn_blocking(move || -> Result<String> {
            // Load on the blocking pool: the heavy model load and its
            // write lock stay off the async runtime thread.
            ensure_loaded(&model_arc, &model_path)?;
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
        .map_err(|e| VoiceTypeError::processing(format!("spawn_blocking: {e}")))?
    }
}

/// 7 arguments are acceptable here because they encapsulate different
/// concerns (model, prompt parts, sampling). A config struct would
/// just be "7 arguments in different packaging".
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
        .ok_or_else(|| VoiceTypeError::processing("LLM model not loaded"))?;
    let backend = LLAMA_BACKEND
        .get()
        .ok_or_else(|| VoiceTypeError::processing("LlamaBackend not initialised"))?;

    // Pull the chat template from the GGUF. Almost all modern models
    // (gemma3, llama3, qwen3, mistral-nemo) bundle a suitable one.
    // `None` = use the default stored in the model.
    let template = model
        .chat_template(None)
        .map_err(|e| VoiceTypeError::processing(format!("chat_template: {e}")))?;

    let messages = vec![
        LlamaChatMessage::new("system".to_string(), system_prompt)
            .map_err(|e| VoiceTypeError::processing(format!("LlamaChatMessage system: {e}")))?,
        LlamaChatMessage::new("user".to_string(), transcript)
            .map_err(|e| VoiceTypeError::processing(format!("LlamaChatMessage user: {e}")))?,
    ];
    // add_ass=true: the template ends with the assistant opening tag,
    // so the model doesn't have to generate an "assistant:" header
    // itself.
    let prompt = model
        .apply_chat_template(&template, &messages, true)
        .map_err(|e| VoiceTypeError::processing(format!("apply_chat_template: {e}")))?;

    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| VoiceTypeError::processing(format!("str_to_token: {e}")))?;

    let prompt_len = tokens.len() as i32;
    if prompt_len > 4000 {
        // Default ctx 4096; over 4000 prompt tokens leaves <100
        // tokens for the answer. The dictation use case should never
        // hit this, but we check defensively.
        return Err(VoiceTypeError::processing(format!(
            "Prompt too long ({prompt_len} tokens) — shorten the dictation or increase ctx_size"
        )));
    }

    let ctx_params = LlamaContextParams::default();
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| VoiceTypeError::processing(format!("new_context: {e}")))?;

    // Build the batch for the prompt. Logits only for the last token,
    // because we only need the next token's prediction.
    let mut batch = LlamaBatch::new(prompt_len as usize + max_tokens as usize, 1);
    let last_idx = prompt_len - 1;
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i as i32 == last_idx;
        batch
            .add(token, i as i32, &[0], is_last)
            .map_err(|e| VoiceTypeError::processing(format!("batch.add prompt: {e}")))?;
    }
    ctx.decode(&mut batch)
        .map_err(|e| VoiceTypeError::processing(format!("decode prompt: {e}")))?;

    // Sampler chain: penalties → top_p → temperature → dist. At
    // temperature == 0 we use greedy instead of dist, for strictly
    // deterministic outputs (rewriting use case).
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
            // Deterministic seed (0): with temp > 0 still stochastic
            // *within* the reduced distribution, but reproducible
            // across app starts.
            LlamaSampler::dist(0),
        ])
    };

    let mut output = String::new();
    let mut cursor = prompt_len;
    // `cursor` is the KV-cache position (starts at prompt_len), not a plain
    // loop counter — explicit_counter_loop is a false positive here.
    #[allow(clippy::explicit_counter_loop)]
    for _ in 0..max_tokens {
        // -1 = last token in the batch (the one set with logits=true).
        let token = sampler.sample(&ctx, -1);
        if model.is_eog_token(token) {
            break;
        }
        // Special::Plaintext = special tokens (chat tags etc.) are
        // dropped; only "visible" text comes out.
        let piece = model
            .token_to_str(token, Special::Plaintext)
            .map_err(|e| VoiceTypeError::processing(format!("token_to_str: {e}")))?;
        output.push_str(&piece);

        // Feed the next decode with the sampled token.
        batch.clear();
        batch
            .add(token, cursor, &[0], true)
            .map_err(|e| VoiceTypeError::processing(format!("batch.add gen: {e}")))?;
        cursor += 1;
        ctx.decode(&mut batch)
            .map_err(|e| VoiceTypeError::processing(format!("decode gen: {e}")))?;
    }

    Ok(output.trim().to_string())
}
