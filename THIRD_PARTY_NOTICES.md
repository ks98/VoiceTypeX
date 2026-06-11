# Third-Party Notices

VoiceTypeX itself is licensed under the **GNU General Public License
Version 3 or later** (`GPL-3.0-or-later`, see [LICENSE](LICENSE)). This
file lists the embedded or runtime-loaded third-party components with
their respective licenses.

## Contents

- [Models (loaded at runtime)](#models-loaded-at-runtime)
- [Embedded libraries (Rust crates / C++)](#embedded-libraries-rust-crates--c)
- [System libraries (linker / dlopen time)](#system-libraries-linker--dlopen-time)
- [Optional external services](#optional-external-services)

---

## Models (loaded at runtime)

These models are **not** shipped in the installer. They are downloaded
from Hugging Face to `~/.config/de.kevin-stenzel.voicetypex/models/` on first setup
(SHA-256-verified) and stored locally there. Each model is governed by
its respective upstream license.

### STT — Speech-to-Text (Whisper family)

| Model | Source | License |
|---|---|---|
| `ggml-large-v3-turbo-q8_0.bin` (default) | [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) | [MIT](https://github.com/openai/whisper/blob/main/LICENSE) (OpenAI Whisper, MIT) |
| `ggml-large-v3-turbo-q5_0.bin` | ditto | MIT |
| `ggml-small-q5_1.bin` | ditto | MIT |
| `ggml-large-v3-turbo.bin` (F16) | ditto | MIT |
| `ggml-model-q5_0.bin` (DE fine-tune) | [cstr/whisper-large-v3-turbo-german-ggml](https://huggingface.co/cstr/whisper-large-v3-turbo-german-ggml) — re-pack of [primeline/whisper-large-v3-turbo-german](https://huggingface.co/primeline/whisper-large-v3-turbo-german) | [Apache 2.0](https://huggingface.co/primeline/whisper-large-v3-turbo-german/blob/main/LICENSE) |
| `ggml-large-v3-turbo-german-q8_0.bin` (DE fine-tune, Q8) | [Pomni/whisper-large-v3-turbo-german-ggml-allquants](https://huggingface.co/Pomni/whisper-large-v3-turbo-german-ggml-allquants) — Q8 re-pack of the same [primeline/whisper-large-v3-turbo-german](https://huggingface.co/primeline/whisper-large-v3-turbo-german) fine-tune (verified: Pomni's Q5_0 LFS oid `15e92e3d…d9d030` is byte-identical to cstr's) | [Apache 2.0](https://huggingface.co/primeline/whisper-large-v3-turbo-german/blob/main/LICENSE) |

### VAD — Voice Activity Detection

| Model | Source | License |
|---|---|---|
| `ggml-silero-v6.2.0.bin` | [ggml-org/whisper-vad](https://huggingface.co/ggml-org/whisper-vad) — re-pack of [snakers4/silero-vad](https://github.com/snakers4/silero-vad) | [MIT](https://github.com/snakers4/silero-vad/blob/master/LICENSE) |

### LLM — Embedded llama-cpp-2 (Phase 3b)

| Model | Source | License |
|---|---|---|
| `gemma-4-E4B-it-Q5_K_M.gguf` (Pro default, May 2026) | [unsloth/gemma-4-E4B-it-GGUF](https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF) | [Gemma Terms of Use](https://ai.google.dev/gemma/terms) — Apache-2.0-compatible use, with a prohibited-use policy |
| `gemma-4-E2B-it-Q5_K_M.gguf` (mid tier) | [unsloth/gemma-4-E2B-it-GGUF](https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF) | Gemma Terms of Use |
| `gemma-3-4b-it-Q5_K_M.gguf` (legacy Pro) | [unsloth/gemma-3-4b-it-GGUF](https://huggingface.co/unsloth/gemma-3-4b-it-GGUF) | Gemma Terms of Use |
| `gemma-3-1b-it-Q5_K_M.gguf` (light default) | [unsloth/gemma-3-1b-it-GGUF](https://huggingface.co/unsloth/gemma-3-1b-it-GGUF) | Gemma Terms of Use |
| `Llama-3.2-1B-Instruct-Q5_K_M.gguf` | [unsloth/Llama-3.2-1B-Instruct-GGUF](https://huggingface.co/unsloth/Llama-3.2-1B-Instruct-GGUF) | [Llama 3.2 Community License](https://www.llama.com/llama3_2/license/) |
| `qwen2.5-1.5b-instruct-q5_k_m.gguf` | [Qwen/Qwen2.5-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF) | [Apache 2.0](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/blob/main/LICENSE) |

**Note on the Gemma license:** Google Gemma is governed by the
[Gemma Terms of Use](https://ai.google.dev/gemma/terms) — a permissive
license similar to Apache 2.0, but with a
[prohibited-use policy](https://ai.google.dev/gemma/prohibited_use_policy)
(e.g. no generation of malware or targeted discrimination). This is an
end-user obligation — VoiceTypeX itself makes no assumptions about use
cases.

**Note on the Llama license:** Llama 3.2 has a
[Community License](https://www.llama.com/llama3_2/license/) with an
Acceptable Use Policy + a 700M-MAU threshold (above the threshold you
need a commercial license agreement with Meta). Unproblematic for
typical dictation use cases.

---

## Embedded libraries (Rust crates / C++)

VoiceTypeX compiles and links the following third-party components:

### Inference engines

| Component | Version | License |
|---|---|---|
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (via `whisper-rs-sys`) | 0.15.x | [MIT](https://github.com/ggml-org/whisper.cpp/blob/master/LICENSE) |
| [whisper-rs](https://github.com/tazz4843/whisper-rs) | 0.16 | [Unlicense OR MIT](https://github.com/tazz4843/whisper-rs/blob/master/LICENSE-MIT) |
| [llama.cpp](https://github.com/ggml-org/llama.cpp) (via `llama-cpp-sys-2`) | bundled in 0.1.146 | [MIT](https://github.com/ggml-org/llama.cpp/blob/master/LICENSE) |
| [llama-cpp-rs](https://github.com/utilityai/llama-cpp-rs) | 0.1.146 | [MIT OR Apache-2.0](https://github.com/utilityai/llama-cpp-rs/blob/main/LICENSE-MIT) |
| [ggml](https://github.com/ggml-org/ggml) (in whisper.cpp + llama.cpp) | ditto | MIT |

`llama-cpp-2` is linked with the `dynamic-link` feature — the resulting
`libllama.so` and `libggml-*.so` are shipped with the bundle and are
likewise MIT.

### Framework + standard crates

| Component | License |
|---|---|
| [Tauri 2](https://tauri.app/) | Apache-2.0 OR MIT |
| [React 18](https://react.dev/) | MIT |
| [TailwindCSS](https://tailwindcss.com/) | MIT |
| [Zustand](https://github.com/pmndrs/zustand) | MIT |
| [tokio](https://tokio.rs/) | MIT |
| [reqwest](https://github.com/seanmonstar/reqwest) | Apache-2.0 OR MIT |
| [serde](https://serde.rs/) / [serde_json](https://github.com/serde-rs/json) | Apache-2.0 OR MIT |
| [hound](https://github.com/ruuda/hound) (WAV encoding) | Apache-2.0 |
| [cpal](https://github.com/RustAudio/cpal) (audio capture) | Apache-2.0 |
| [rubato](https://github.com/HEnquist/rubato) (resampling) | MIT |
| [rodio](https://github.com/RustAudio/rodio) (audio playback) | MIT OR Apache-2.0 |
| [enigo](https://github.com/enigo-rs/enigo) (keystroke injection) | MIT |
| [reis](https://github.com/ids1024/reis) (libei Wayland) | MIT OR Apache-2.0 |
| [ashpd](https://github.com/bilelmoussaoui/ashpd) (xdg-portal) | MIT |

A complete crate license list can be generated with:
```bash
cargo install cargo-license
cargo license --manifest-path src-tauri/Cargo.toml
```

---

## System libraries (linker / dlopen time)

These system components are required at build time or loaded at runtime
via standard OS paths — they are **not** shipped with the bundle.

| Component | License | When needed |
|---|---|---|
| Vulkan loader (`libvulkan1`) | Apache-2.0 | default build; runtime via driver |
| Mesa Vulkan Drivers (llvmpipe + hardware backends) | MIT | runtime on systems without a vendor GPU driver |
| GTK 3 / WebKit2GTK / libsoup | LGPL-2.1+ (dynamically linked) | Tauri webview, Linux |
| OpenBLAS (optional, `fast-cpu` feature) | BSD-3-Clause | only when built without Vulkan |
| WebView2 Runtime | proprietary (Microsoft) | Windows, shipped by Tauri |
| NVIDIA CUDA Runtime (`libcudart.so.*`) | NVIDIA EULA | **not bundled** — the user installs it themselves; the app uses it via dlopen if present (Task #27, feature `embedded-cuda-dynamic`) |

**CUDA note:** We bundle **no** parts of the NVIDIA CUDA SDK. The
`embedded-cuda-dynamic` build variant loads the user's CUDA runtime via
`dlopen` at runtime. This keeps the GPL-3 bundle free of the NVIDIA EULA
(see the Phase-3 architecture research in the repo commit history).

---

## Optional external services

These are used only if the user explicitly configures them. API keys are
stored locally (chmod 0600) and never go into logs.

| Service | License / TOS | Usage |
|---|---|---|
| [Ollama](https://ollama.com/) (local daemon) | MIT (code) | Legacy LLM path, opt-in via `local_engine = "ollama"` |
| [xAI API](https://x.ai/api) | xAI ToS | Cloud STT (Grok-STT) + LLM (Grok-4-fast) |
| [OpenAI API](https://openai.com/api/) | OpenAI ToS | Cloud STT (Whisper API) + LLM (GPT) |
| [Anthropic API](https://www.anthropic.com/) | Anthropic ToS | Cloud LLM (Claude) |
| [Groq API](https://groq.com/) | Groq ToS | Cloud STT (Whisper-Turbo) |
| [Deepgram API](https://deepgram.com/) | Deepgram ToS | Cloud STT (Nova-3) |

---

## Full license texts

- VoiceTypeX (GPL-3.0-or-later): [LICENSE](LICENSE)
- Full texts of all embedded licenses are available from the upstream
  repos linked above.

For questions about the license compatibility of individual components:
[mail@kevin-stenzel.de](mailto:mail@kevin-stenzel.de).
