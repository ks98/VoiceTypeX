# Third-Party Notices

VoiceTypeX selbst steht unter der **GNU General Public License Version 3
oder spaeter** (`GPL-3.0-or-later`, siehe [LICENSE](LICENSE)). Diese
Datei listet die eingebetteten oder zur Laufzeit nachgeladenen
Komponenten Dritter mit ihren jeweiligen Lizenzen.

## Inhalt

- [Modelle (zur Laufzeit nachgeladen)](#modelle-zur-laufzeit-nachgeladen)
- [Eingebettete Bibliotheken (Rust-Crates / C++)](#eingebettete-bibliotheken-rust-crates--c)
- [System-Bibliotheken (Linker-/dlopen-Zeit)](#system-bibliotheken-linker--dlopen-zeit)
- [Optionale externe Dienste](#optionale-externe-dienste)

---

## Modelle (zur Laufzeit nachgeladen)

Diese Modelle werden **nicht** im Installer mitgeliefert. Sie werden
beim ersten Setup von Hugging Face nach `~/.config/voicetypex/models/`
geladen (SHA-256-verifiziert) und sind dort lokal gespeichert. Pro
Modell gilt die jeweilige Upstream-Lizenz.

### STT — Speech-to-Text (Whisper-Familie)

| Modell | Quelle | Lizenz |
|---|---|---|
| `ggml-large-v3-turbo-q8_0.bin` (Default) | [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) | [MIT](https://github.com/openai/whisper/blob/main/LICENSE) (OpenAI Whisper, MIT) |
| `ggml-large-v3-turbo-q5_0.bin` | dito | MIT |
| `ggml-small-q5_1.bin` | dito | MIT |
| `ggml-large-v3-turbo.bin` (F16) | dito | MIT |
| `ggml-model-q5_0.bin` (DE-Fine-tune) | [cstr/whisper-large-v3-turbo-german-ggml](https://huggingface.co/cstr/whisper-large-v3-turbo-german-ggml) — Re-Pack von [primeline/whisper-large-v3-turbo-german](https://huggingface.co/primeline/whisper-large-v3-turbo-german) | [Apache 2.0](https://huggingface.co/primeline/whisper-large-v3-turbo-german/blob/main/LICENSE) |

### VAD — Voice Activity Detection

| Modell | Quelle | Lizenz |
|---|---|---|
| `ggml-silero-v6.2.0.bin` | [ggml-org/whisper-vad](https://huggingface.co/ggml-org/whisper-vad) — Re-Pack von [snakers4/silero-vad](https://github.com/snakers4/silero-vad) | [MIT](https://github.com/snakers4/silero-vad/blob/master/LICENSE) |

### LLM — Embedded llama-cpp-2 (Phase 3b)

| Modell | Quelle | Lizenz |
|---|---|---|
| `gemma-4-E4B-it-Q5_K_M.gguf` (Pro-Default, Mai 2026) | [unsloth/gemma-4-E4B-it-GGUF](https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF) | [Gemma Terms of Use](https://ai.google.dev/gemma/terms) — Apache-2.0-compatible Use, mit Prohibited-Use-Policy |
| `gemma-4-E2B-it-Q5_K_M.gguf` (Mittel-Tier) | [unsloth/gemma-4-E2B-it-GGUF](https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF) | Gemma Terms of Use |
| `gemma-3-4b-it-Q5_K_M.gguf` (Legacy-Pro) | [unsloth/gemma-3-4b-it-GGUF](https://huggingface.co/unsloth/gemma-3-4b-it-GGUF) | Gemma Terms of Use |
| `gemma-3-1b-it-Q5_K_M.gguf` (Light-Default) | [unsloth/gemma-3-1b-it-GGUF](https://huggingface.co/unsloth/gemma-3-1b-it-GGUF) | Gemma Terms of Use |
| `Llama-3.2-1B-Instruct-Q5_K_M.gguf` | [unsloth/Llama-3.2-1B-Instruct-GGUF](https://huggingface.co/unsloth/Llama-3.2-1B-Instruct-GGUF) | [Llama 3.2 Community License](https://www.llama.com/llama3_2/license/) |
| `qwen2.5-1.5b-instruct-q5_k_m.gguf` | [Qwen/Qwen2.5-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF) | [Apache 2.0](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/blob/main/LICENSE) |

**Hinweis zu Gemma-Lizenz:** Google Gemma steht unter den
[Gemma Terms of Use](https://ai.google.dev/gemma/terms) — eine
permissive Lizenz aehnlich Apache 2.0, aber mit einer
[Prohibited-Use-Policy](https://ai.google.dev/gemma/prohibited_use_policy)
(z.B. keine Generierung von Schadsoftware oder gezielter
Diskriminierung). End-User-Pflicht — VoiceTypeX selbst macht keine
Annahmen ueber Use-Cases.

**Hinweis zu Llama-Lizenz:** Llama 3.2 hat eine
[Community License](https://www.llama.com/llama3_2/license/) mit
Acceptable Use Policy + 700-Mio-MAU-Schwelle (oberhalb der Schwelle
muss man einen kommerziellen Lizenz-Vertrag mit Meta haben). Fuer
typische Diktier-Use-Cases unproblematisch.

---

## Eingebettete Bibliotheken (Rust-Crates / C++)

VoiceTypeX kompiliert und linkt folgende Komponenten Dritter:

### Inference-Engines

| Komponente | Version | Lizenz |
|---|---|---|
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (via `whisper-rs-sys`) | 0.15.x | [MIT](https://github.com/ggml-org/whisper.cpp/blob/master/LICENSE) |
| [whisper-rs](https://github.com/tazz4843/whisper-rs) | 0.16 | [Unlicense OR MIT](https://github.com/tazz4843/whisper-rs/blob/master/LICENSE-MIT) |
| [llama.cpp](https://github.com/ggml-org/llama.cpp) (via `llama-cpp-sys-2`) | bundled in 0.1.146 | [MIT](https://github.com/ggml-org/llama.cpp/blob/master/LICENSE) |
| [llama-cpp-rs](https://github.com/utilityai/llama-cpp-rs) | 0.1.146 | [MIT OR Apache-2.0](https://github.com/utilityai/llama-cpp-rs/blob/main/LICENSE-MIT) |
| [ggml](https://github.com/ggml-org/ggml) (in whisper.cpp + llama.cpp) | dito | MIT |

`llama-cpp-2` wird mit Feature `dynamic-link` gelinkt — die
resultierenden `libllama.so` und `libggml-*.so` werden mit dem
Bundle ausgeliefert und sind ebenfalls MIT.

### Framework + Standard-Crates

| Komponente | Lizenz |
|---|---|
| [Tauri 2](https://tauri.app/) | Apache-2.0 OR MIT |
| [React 18](https://react.dev/) | MIT |
| [TailwindCSS](https://tailwindcss.com/) | MIT |
| [Zustand](https://github.com/pmndrs/zustand) | MIT |
| [tokio](https://tokio.rs/) | MIT |
| [reqwest](https://github.com/seanmonstar/reqwest) | Apache-2.0 OR MIT |
| [serde](https://serde.rs/) / [serde_json](https://github.com/serde-rs/json) | Apache-2.0 OR MIT |
| [hound](https://github.com/ruuda/hound) (WAV-Encoding) | Apache-2.0 |
| [cpal](https://github.com/RustAudio/cpal) (Audio-Capture) | Apache-2.0 |
| [rubato](https://github.com/HEnquist/rubato) (Resampling) | MIT |
| [rodio](https://github.com/RustAudio/rodio) (Audio-Playback) | MIT OR Apache-2.0 |
| [enigo](https://github.com/enigo-rs/enigo) (Keystroke-Injection) | MIT |
| [reis](https://github.com/ids1024/reis) (libei-Wayland) | MIT OR Apache-2.0 |
| [ashpd](https://github.com/bilelmoussaoui/ashpd) (xdg-portal) | MIT |

Eine vollstaendige Crate-Lizenz-Liste laesst sich erzeugen mit:
```bash
cargo install cargo-license
cargo license --manifest-path src-tauri/Cargo.toml
```

---

## System-Bibliotheken (Linker-/dlopen-Zeit)

Diese System-Komponenten werden zur Build-Zeit benoetigt oder
zur Laufzeit ueber Standard-OS-Pfade geladen — **nicht** mit dem
Bundle ausgeliefert.

| Komponente | Lizenz | Wann benoetigt |
|---|---|---|
| Vulkan-Loader (`libvulkan1`) | Apache-2.0 | Default-Build; Runtime via Treiber |
| Mesa Vulkan Drivers (llvmpipe + Hardware-Backends) | MIT | Runtime auf Systemen ohne Vendor-GPU-Treiber |
| GTK 3 / WebKit2GTK / libsoup | LGPL-2.1+ (dynamisch gelinkt) | Tauri-Webview, Linux |
| OpenBLAS (optional, `fast-cpu`-Feature) | BSD-3-Clause | nur wenn ohne Vulkan gebaut |
| WebView2 Runtime | proprietaer (Microsoft) | Windows, von Tauri ausgeliefert |
| NVIDIA CUDA Runtime (`libcudart.so.*`) | NVIDIA EULA | **nicht gebundlet** — User installiert selbst, App nutzt via dlopen wenn vorhanden (Task #27, Feature `embedded-cuda-dynamic`) |

**CUDA-Hinweis:** Wir buendeln **keine** Teile des NVIDIA CUDA SDK
mit. Die `embedded-cuda-dynamic`-Build-Variante laedt das CUDA-Runtime
des Users via `dlopen` zur Laufzeit. Damit bleibt das GPL-3-Bundle
NVIDIA-EULA-frei (siehe Phase-3-Architektur-Recherche im Repo-
Commit-History).

---

## Optionale externe Dienste

Diese werden nur genutzt, wenn der User sie explizit konfiguriert. API-
Keys liegen lokal (chmod 0600), gehen niemals in Logs.

| Dienst | Lizenz / TOS | Verwendung |
|---|---|---|
| [Ollama](https://ollama.com/) (lokaler Daemon) | MIT (Code) | Legacy-LLM-Pfad, opt-in via `local_engine = "ollama"` |
| [xAI API](https://x.ai/api) | xAI ToS | Cloud-STT (Grok-STT) + LLM (Grok-4) |
| [OpenAI API](https://openai.com/api/) | OpenAI ToS | Cloud-STT (Whisper-API) + LLM (GPT) |
| [Anthropic API](https://www.anthropic.com/) | Anthropic ToS | Cloud-LLM (Claude) |
| [Groq API](https://groq.com/) | Groq ToS | Cloud-STT (Whisper-Turbo) |
| [Deepgram API](https://deepgram.com/) | Deepgram ToS | Cloud-STT (Nova-2) |

---

## Vollstaendige Lizenz-Texte

- VoiceTypeX (GPL-3.0-or-later): [LICENSE](LICENSE)
- Volltexte aller eingebetteten Lizenzen sind aus den oben verlinkten
  Upstream-Repos zu beziehen.

Bei Fragen zu Lizenz-Kompatibilitaet einzelner Komponenten:
[mail@kevin-stenzel.de](mailto:mail@kevin-stenzel.de).
