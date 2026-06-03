# Cloud Providers — Wire Protocols

> As of May 2026. On API drift, pull the official provider docs via
> WebFetch before changing the implementation — this file is a
> snapshot, not a guarantee about the future. Third-party sources (blog
> posts, forum threads) are hints only, never a substitute.

## Overview

| Domain | Provider | File in the code |
|---|---|---|
| STT | xAI | `src-tauri/src/transcription/cloud/xai.rs` |
| STT | OpenAI Whisper | `src-tauri/src/transcription/cloud/openai.rs` (wraps `whisper_compatible.rs`) |
| STT | Groq Whisper | `src-tauri/src/transcription/cloud/groq.rs` (wraps `whisper_compatible.rs`) |
| STT | Deepgram | `src-tauri/src/transcription/cloud/deepgram.rs` |
| LLM | xAI Grok | `src-tauri/src/processing/cloud/xai.rs` (wraps `openai_compatible.rs`) |
| LLM | OpenAI GPT | `src-tauri/src/processing/cloud/openai.rs` (wraps `openai_compatible.rs`) |
| LLM | Anthropic Claude | `src-tauri/src/processing/cloud/anthropic.rs` |
| LLM (local) | Ollama | `src-tauri/src/processing/local.rs` |

The decision of when a wrapper is shared and when it isn't follows the
actual protocol kinship: OpenAI and Groq are both
Whisper-API-compatible and share `whisper_compatible.rs`; xAI, OpenAI,
and Groq share the Chat-Completions-compatible path via
`openai_compatible.rs`. Deepgram (STT) and Anthropic (LLM) stand on
their own — no artificial shared wrapper.

## STT Providers

### xAI STT

- **Endpoint:** `POST https://api.x.ai/v1/stt`
- **Auth:** Bearer header (`Authorization: Bearer <api_key>`)
- **Body:** `multipart/form-data`
- **Important:** `file` must be the **last** multipart field.
  Optional fields (e.g. `language`) come before it.
- **No `model` field:** Unlike the Whisper-compatible providers,
  `/v1/stt` accepts **no** `model` and **no** `initial_prompt`
  field (official docs, as of May 2026 — only `file`/`url` plus flags
  like `language`/`diarize`/`keyterm`). Earlier versions sent a
  fabricated `model = "stt-1"`; that has been removed.
- **Response:** `{ text, language, duration, words[] }` — we use
  only `text`.
- **Language forcing:** none. xAI's `language` parameter only controls
  text formatting (numbers/currencies), not speech recognition.
  Recognition is hardcoded to auto-detect — see the "Known
  Limitations" section below.

### OpenAI Whisper / Groq Whisper

Both use OpenAI's Whisper API, or Groq's API-compatible variant.
Shared implementation in `whisper_compatible.rs`:

- **Endpoint (OpenAI):** `POST https://api.openai.com/v1/audio/transcriptions`
- **Endpoint (Groq):** `POST https://api.groq.com/openai/v1/audio/transcriptions`
- **Auth:** Bearer header
- **Body:** `multipart/form-data` with `file` and `model`
- **Model:** OpenAI `whisper-1`, Groq `whisper-large-v3-turbo`
- **Response:** `{ text }` (json format)

### Deepgram

- **Endpoint:** `POST https://api.deepgram.com/v1/listen?model=nova-3&language=…`
- **Auth:** `Authorization: Token <api_key>` (**not** Bearer)
- **Body:** raw audio bytes (Content-Type matching the WAV)
- **Response:** `{ results: { channels: [ { alternatives: [ { transcript } ] } ] } }`

## LLM Providers

### xAI Grok / OpenAI GPT — OpenAI Chat-Completions-compatible

Shared implementation in `openai_compatible.rs`:

- **Endpoint suffix:** `POST {base_url}/chat/completions`
- **Base URLs:** xAI `https://api.x.ai/v1`, OpenAI `https://api.openai.com/v1`
- **Auth:** Bearer header
- **Body:**
  ```json
  {
    "model": "...",
    "messages": [
      { "role": "system", "content": "<system_prompt>" },
      { "role": "user",   "content": "<transcript>" }
    ]
  }
  ```
- **Response path:** `choices[0].message.content`
- **Default models:**
  - xAI: `grok-4-fast-non-reasoning` (postprocessing default —
    no reasoning overhead, ~6× cheaper than `grok-4`, 2 M context).
    `grok-4` is opt-in per mode only, when genuine multi-step reasoning
    is needed.
  - OpenAI: `gpt-4o-mini`.

### Anthropic Claude — standalone

Anthropic uses the Messages API, not Chat Completions:

- **Endpoint:** `POST https://api.anthropic.com/v1/messages`
- **Auth:** `x-api-key: <api_key>` (**not** Bearer)
- **Required header:** `anthropic-version: 2023-06-01`
- **Body:**
  ```json
  {
    "model": "...",
    "system": "<system_prompt>",
    "messages": [
      { "role": "user", "content": "<transcript>" }
    ],
    "max_tokens": 4096
  }
  ```
  - **Note:** `system` is a top-level field, **not** part of the
    `messages` list (unlike the OpenAI-compatibles).
- **Response path:** `content[0].text`

### Embedded LLM via llama-cpp-2 (Phase 3b — default path as of May 2026)

The **embedded** LLM path embeds llama.cpp directly into the VoiceTypeX
process — no external daemon needed. Enabled per mode via
`local_engine = "embedded"` in the mode TOML.

- **Crate:** `llama-cpp-2 = "0.1.146"`, features `vulkan + sampler +
  dynamic-link`. `dynamic-link` is mandatory (otherwise it collides
  with whisper-rs-sys over a statically linked ggml).
- **Backend:** GPU via Vulkan (same as Whisper), CPU fallback.
- **Model format:** GGUF, loaded via `LlamaModel::load_from_file`.
  Path from `Settings.llm_model_path` (override) or slot-based from
  `Settings.llm_default_slot`.
- **Lifecycle:** the model is loaded LAZILY on the first `process()`
  call and cached for the app's lifetime behind
  `Arc<RwLock<Option<LlamaModel>>>`. If the user never uses embedded,
  the GGUF file stays optional.
- **Pipeline per call:**
  1. Chat template from the GGUF: `model.chat_template(None)`.
  2. `LlamaChatMessage` list (system + user) → `apply_chat_template
     (template, msgs, add_ass=true)`.
  3. `model.str_to_token(prompt, AddBos::Always)`.
  4. Fresh `LlamaContext` + `LlamaBatch`. Feed in prompt tokens, one
     `decode()`.
  5. Sampler chain: `penalties → top_p → temp → dist` (or `greedy`
     when temperature == 0).
  6. Token loop until EOG or `max_tokens` (default 1024).
  7. Detokenize via `token_to_str(Special::Plaintext)`.
- **Sampling defaults** (when `None` in the mode TOML): temperature 0.2,
  top_p 0.8, repeat_penalty 1.05, max_tokens 1024.

**GGUF slots** (`LlmModelSlot::from_setting`, refreshed May 2026 with
Gemma 4 as the new Pro/Medium defaults):

| Slot slug | File | Size | Recommendation | Source |
|---|---|---|---|---|
| `gemma4-e4b-it-q5_k_m` | `gemma-4-E4B-it-Q5_K_M.gguf` | ~5.1 GB | **Pro · 12+ GB RAM** | unsloth/gemma-4-E4B-it-GGUF |
| `gemma4-e2b-it-q5_k_m` | `gemma-4-E2B-it-Q5_K_M.gguf` | ~3.1 GB | **Medium · 8-12 GB RAM** | unsloth/gemma-4-E2B-it-GGUF |
| `gemma3-1b-it-q5_k_m` *(Light default)* | `gemma-3-1b-it-Q5_K_M.gguf` | ~851 MB | **Light · <8 GB RAM** | unsloth/gemma-3-1b-it-GGUF |
| `gemma3-4b-it-q5_k_m` | `gemma-3-4b-it-Q5_K_M.gguf` | ~2.8 GB | Legacy Pro (Phase 1) | unsloth/gemma-3-4b-it-GGUF |
| `llama3.2-1b-instruct-q5_k_m` | `Llama-3.2-1B-Instruct-Q5_K_M.gguf` | ~912 MB | Light, EN-focused | unsloth/Llama-3.2-1B-Instruct-GGUF |
| `qwen2.5-1.5b-instruct-q5_k_m` | `qwen2.5-1.5b-instruct-q5_k_m.gguf` | ~1.3 GB | Code-friendly | Qwen/Qwen2.5-1.5B-Instruct-GGUF |

**Gemma 4 (April 2026, Apache 2.0)** is the preferred model baseline
for the embedded LLM. Matformer architecture ("effective" vs "total"
params: E4B has 4.5 B effective at 7B+ raw → larger disk files than the
naive param count suggests). 256k context window, 140+ languages,
multimodal-capable (text/image/audio — we use text only). llama.cpp
support including chat-template updates since April 2026.

**Gemma 3 stays** for the Light tier (4 GB setups) and as a backward-
compat path for users who want to stick with the smaller disk size.

All with pinned SHA-256 hashes; download via `download_llm()` in
`transcription/model_downloader.rs` with in-flight verification.
unsloth re-packs are preferred because the bartowski/google original
repos have a license gate (Gemma acceptance on first download).

**Known build quirk (automated):** llama-cpp-sys-2 0.1.146's
build.rs has a TOC/TOU bug with dangling symlinks in `target/
debug/`. The `predev`/`prebuild` hook in `package.json`
(`scripts/clean-dangling-libs.mjs`) cleans this up automatically before
every Tauri build. See PLATFORMS.md for details.

**NVIDIA builder path (Phase 3b, Task #27 — opt-in):**
Builders with the CUDA toolkit + libvulkan-dev can produce a bundle
with runtime backend dispatch:
```bash
cargo build --release --features embedded-cuda-dynamic
```
The feature enables llama-cpp-2's `dynamic-backends` feature: GGML
builds CPU/Vulkan/CUDA as separate `ggml-*.so` files. At runtime,
llama.cpp loads the available backends via `ggml_backend_load_all()`
and uses the fastest (typically CUDA when `libcudart.so.13` is present
on the user's system, otherwise Vulkan, otherwise CPU).

**Important for license cleanliness:** the CUDA runtime is **not**
bundled (NVIDIA EULA conflict with GPL-3 — see the Phase 3 architecture
research). The user must have the CUDA toolkit, or at least the
`cuda-runtime` packages, installed themselves. Without the CUDA runtime,
the path transparently falls back to Vulkan. The `dynamic-backends`
build variant is therefore a hybrid: NVIDIA users with a CUDA driver
get full speed, everyone else is served via Vulkan — all from a single
bundle. The standard default build (without the feature) is Vulkan-only
and needs no CUDA toolkit on the build machine.

### Ollama (local, no BYOK key — opt-in)

Embedded has been the **default path** for `processing = "local"` since
May 2026. Ollama remains as an opt-in for users who want to keep using
their existing daemon installation, or who run models that aren't yet
slot-selectable as GGUF on the embedded path. Enabled per mode via
`local_engine = "ollama"`. Existing Phase 1/2 TOMLs without an explicit
`local_engine` are automatically set to `"ollama"` in
`Mode::migrate_deprecated_fields` (verified by tests in `core/modes.rs`).

- **Endpoint (default):** `POST http://127.0.0.1:11434/api/chat`
- **Auth:** none (local HTTP server)
- **Default model (as of May 2026):** `gemma3:4b` (previously `qwen2.5:7b`)
  — Gemma 3 4B-IT from DeepMind, ~3 GB footprint, 140+ languages, very
  strong on German. Switch via `Mode.ollama_model_tag` per mode
  (the deprecated alias `local_llm_model` is auto-migrated on load).
- **Body:** chat format analogous to OpenAI, plus Ollama-specific
  fields:
  ```json
  {
    "model": "...",
    "messages": [
      { "role": "system", "content": "<system_prompt>" },
      { "role": "user",   "content": "<transcript>" }
    ],
    "stream": false,
    "keep_alive": "5m",
    "options": {
      "temperature": 0.2,
      "top_p": 0.8,
      "repeat_penalty": 1.05
    }
  }
  ```
- **`keep_alive`:** duration string, controls how long Ollama keeps the
  model in RAM/VRAM after the call. From
  `Settings.ollama_keep_alive` (default `"5m"`). `"0"` = immediate
  unload (memory-pressure profile), `"-1"` = keep warm indefinitely.
- **`options.{temperature,top_p,repeat_penalty}`:** all from the
  mode TOML; `None` = Ollama server default. Recommendation for "faithful
  rewrite, do not extend": 0.2 / 0.8 / 1.05.
- **Response path:** `message.content`
- **Timeout:** 300 s (local inference can take a while on CPU)
- **Endpoint overridable:** the `ollama_url` settings field.

## Secret Handling

API keys live per provider in the file `~/.config/.../secrets.json`
(chmod 0600) plus, best-effort, in the OS keychain. **xAI is a special
case:** a single entry covers both STT *and* LLM, because both endpoints
use the same key.

Keys are **never** logged — logging shows only the
key length for diagnostics. Provider requests go exclusively through
the Rust backend; the key never leaves the process for the frontend
(the `get_provider_status` IPC returns only
`{ configured: bool, error: Option<String> }`).

## Known Limitations

### xAI STT — no language forcing

xAI's STT API accepts no parameter to pin down language recognition.
The `language` field in the request only controls text formatting (e.g.
how numbers and currencies are spelled), not acoustic speech
recognition. Recognition is hardcoded to auto-detect server-side.

**Practical consequence:** for short, language-neutral dictations
(e.g. isolated proper nouns, technical terms, short commands) the
model can guess wrong — for instance, interpreting a German command as
English and transcribing it phonetically.

**Workaround:** switch to local Whisper STT (`transcription = "local"`
in the mode); there you can force `language = "de"`. For longer, clearly
German dictations, xAI is robust in practice, so we accept the
limitation for the cloud modes and plan **no** fix (it would have to
happen on the provider's API side and is out of our hands).

**Sources:** behavior observed in our own practice (as of May 2026);
xAI has not officially documented the behavior as an API constraint, so
a later change is possible at any time.

## When you add a new provider

1. Pull the official provider docs via WebFetch — don't rely on this
   file or on similar-looking APIs.
2. Check whether it fits structurally into an existing wrapper
   (`whisper_compatible.rs` for Whisper-API-compatible STT,
   `openai_compatible.rs` for Chat-Completions-compatible LLM).
   Fold it into the wrapper **only** when the kinship is real —
   otherwise a standalone file like Deepgram / Anthropic.
3. Look closely at the auth mode: Bearer is common, but Deepgram uses
   `Token`, Anthropic `x-api-key`. Mind required headers
   (`anthropic-version`).
4. Add the secret entry in `core/config.rs` + extend the factory in
   `transcription/mod.rs` or `processing/mod.rs`.
5. Tests for response parsing (mock response → `text` extraction).
