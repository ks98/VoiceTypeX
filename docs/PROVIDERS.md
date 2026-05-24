# Cloud-Provider — Wire-Protokolle

> Stand: Mai 2026. Bei API-Drift vor Implementierungs-Änderungen die
> offizielle Provider-Doku via WebFetch ziehen — diese Datei ist eine
> Momentaufnahme, keine Zukunfts-Garantie. Drittquellen (Blog-Posts,
> Forum-Threads) sind nur Hinweise, nie Ersatz.

## Überblick

| Domäne | Provider | Datei im Code |
|---|---|---|
| STT | xAI | `src-tauri/src/transcription/cloud/xai.rs` |
| STT | OpenAI Whisper | `src-tauri/src/transcription/cloud/openai.rs` (Wrap um `whisper_compatible.rs`) |
| STT | Groq Whisper | `src-tauri/src/transcription/cloud/groq.rs` (Wrap um `whisper_compatible.rs`) |
| STT | Deepgram | `src-tauri/src/transcription/cloud/deepgram.rs` |
| LLM | xAI Grok | `src-tauri/src/processing/cloud/xai.rs` (Wrap um `openai_compatible.rs`) |
| LLM | OpenAI GPT | `src-tauri/src/processing/cloud/openai.rs` (Wrap um `openai_compatible.rs`) |
| LLM | Anthropic Claude | `src-tauri/src/processing/cloud/anthropic.rs` |
| LLM (lokal) | Ollama | `src-tauri/src/processing/local.rs` |

Die Wahl, wann ein Wrapper geteilt wird und wann nicht, folgt der
realen Protokoll-Verwandtschaft (CLAUDE.md §4.6): OpenAI und Groq
sind beide Whisper-API-kompatibel und teilen `whisper_compatible.rs`;
xAI, OpenAI und Groq teilen sich Chat-Completions-kompatibel über
`openai_compatible.rs`. Deepgram (STT) und Anthropic (LLM) sind
eigenständig — kein künstlicher Shared-Wrapper.

## STT-Provider

### xAI STT

- **Endpoint:** `POST https://api.x.ai/v1/stt`
- **Auth:** Bearer-Header (`Authorization: Bearer <api_key>`)
- **Body:** `multipart/form-data`
- **Wichtig:** `file` muss das **letzte** Multipart-Feld sein.
  Andere Felder (z.B. `model`, `response_format`) davor.
- **Response:** `{ text, language, duration, words[] }` — wir nutzen
  nur `text`.
- **Sprach-Erzwingung:** keine. xAI's `language`-Parameter steuert nur
  Text-Formatting (Zahlen/Währungen), nicht die Spracherkennung.
  Die Erkennung ist hartcodiert auto-detect — siehe Abschnitt
  „Bekannte Limitierungen" unten.

### OpenAI Whisper / Groq Whisper

Beide nutzen die Whisper-API von OpenAI bzw. Groqs API-kompatible Variante.
Gemeinsame Implementierung in `whisper_compatible.rs`:

- **Endpoint (OpenAI):** `POST https://api.openai.com/v1/audio/transcriptions`
- **Endpoint (Groq):** `POST https://api.groq.com/openai/v1/audio/transcriptions`
- **Auth:** Bearer-Header
- **Body:** `multipart/form-data` mit `file` und `model`
- **Model:** OpenAI `whisper-1`, Groq `whisper-large-v3-turbo`
- **Response:** `{ text }` (json-Format)

### Deepgram

- **Endpoint:** `POST https://api.deepgram.com/v1/listen?model=nova-3&language=…`
- **Auth:** `Authorization: Token <api_key>` (**nicht** Bearer)
- **Body:** Raw-Audio-Bytes (Content-Type passend zum WAV)
- **Response:** `{ results: { channels: [ { alternatives: [ { transcript } ] } ] } }`

## LLM-Provider

### xAI Grok / OpenAI GPT — OpenAI-Chat-Completions-kompatibel

Gemeinsame Implementierung in `openai_compatible.rs`:

- **Endpoint-Suffix:** `POST {base_url}/chat/completions`
- **Base-URLs:** xAI `https://api.x.ai/v1`, OpenAI `https://api.openai.com/v1`
- **Auth:** Bearer-Header
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
- **Response-Pfad:** `choices[0].message.content`
- **Default-Models:**
  - xAI: `grok-4-fast-non-reasoning` (Postprocessing-Default —
    kein Reasoning-Overhead, ~6× günstiger als `grok-4`, 2 M Context).
    `grok-4` nur opt-in pro Modus, wenn echtes Multi-Step-Reasoning
    gebraucht wird.
  - OpenAI: `gpt-4o-mini`.

### Anthropic Claude — eigenständig

Anthropic nutzt die Messages-API, nicht Chat-Completions:

- **Endpoint:** `POST https://api.anthropic.com/v1/messages`
- **Auth:** `x-api-key: <api_key>` (**nicht** Bearer)
- **Pflicht-Header:** `anthropic-version: 2023-06-01`
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
  - **Achtung:** `system` ist Top-Level-Feld, **nicht** Teil der
    `messages`-Liste (anders als bei OpenAI-Kompatiblen).
- **Response-Pfad:** `content[0].text`

### Embedded LLM via llama-cpp-2 (Phase 3b — Default-Pfad ab Mai 2026)

Der **embedded** LLM-Pfad bettet llama.cpp direkt in den VoiceTypeX-
Prozess ein — kein externer Daemon noetig. Aktivierung pro Modus via
`local_engine = "embedded"` in der Mode-TOML.

- **Crate:** `llama-cpp-2 = "0.1.146"`, Features `vulkan + sampler +
  dynamic-link`. `dynamic-link` ist Pflicht (kollidiert sonst mit
  whisper-rs-sys bei statischer ggml).
- **Backend:** GPU via Vulkan (gleich wie Whisper), CPU-Fallback.
- **Modell-Format:** GGUF, geladen ueber `LlamaModel::load_from_file`.
  Pfad aus `Settings.llm_model_path` (Override) oder slot-basiert aus
  `Settings.llm_default_slot`.
- **Lifecycle:** Modell wird LAZY beim ersten `process()`-Call geladen,
  cached fuer App-Lebenszeit hinter `Arc<RwLock<Option<LlamaModel>>>`.
  Wenn der User Embedded nie nutzt, bleibt die GGUF-Datei optional.
- **Pipeline pro Call:**
  1. Chat-Template aus dem GGUF: `model.chat_template(None)`.
  2. `LlamaChatMessage`-Liste (system + user) → `apply_chat_template
     (template, msgs, add_ass=true)`.
  3. `model.str_to_token(prompt, AddBos::Always)`.
  4. Frischer `LlamaContext` + `LlamaBatch`. Prompt-Tokens rein, einmal
     `decode()`.
  5. Sampler-Kette: `penalties → top_p → temp → dist` (oder `greedy`
     bei temperature == 0).
  6. Token-Loop bis EOG oder `max_tokens` (Default 1024).
  7. Detokenize via `token_to_str(Special::Plaintext)`.
- **Sampling-Defaults** (bei `None` in Mode-TOML): temperature 0.2,
  top_p 0.8, repeat_penalty 1.05, max_tokens 1024.

**GGUF-Slots** (`LlmModelSlot::from_setting`, Refresh Mai 2026 mit
Gemma 4 als neue Pro/Mittel-Defaults):

| Slot-Slug | Datei | Größe | Empfehlung | Quelle |
|---|---|---|---|---|
| `gemma4-e4b-it-q5_k_m` | `gemma-4-E4B-it-Q5_K_M.gguf` | ~5,1 GB | **Pro · 12+ GB RAM** | unsloth/gemma-4-E4B-it-GGUF |
| `gemma4-e2b-it-q5_k_m` | `gemma-4-E2B-it-Q5_K_M.gguf` | ~3,1 GB | **Mittel · 8-12 GB RAM** | unsloth/gemma-4-E2B-it-GGUF |
| `gemma3-1b-it-q5_k_m` *(Light-Default)* | `gemma-3-1b-it-Q5_K_M.gguf` | ~851 MB | **Light · <8 GB RAM** | unsloth/gemma-3-1b-it-GGUF |
| `gemma3-4b-it-q5_k_m` | `gemma-3-4b-it-Q5_K_M.gguf` | ~2,8 GB | Legacy-Pro (Phase 1) | unsloth/gemma-3-4b-it-GGUF |
| `llama3.2-1b-instruct-q5_k_m` | `Llama-3.2-1B-Instruct-Q5_K_M.gguf` | ~912 MB | Light, EN-fokussiert | unsloth/Llama-3.2-1B-Instruct-GGUF |
| `qwen2.5-1.5b-instruct-q5_k_m` | `qwen2.5-1.5b-instruct-q5_k_m.gguf` | ~1,3 GB | Code-affin | Qwen/Qwen2.5-1.5B-Instruct-GGUF |

**Gemma 4 (April 2026, Apache 2.0)** ist der bevorzugte Modell-Stand
fuer Embedded-LLM. Matformer-Architektur ("effective" vs "total"
params: E4B hat 4,5 B effective bei 7B+ raw → groessere Disk-Files als
naive Param-Zahl vermuten laesst). 256k Context-Window, 140+
Sprachen, multimodal-faehig (Text/Image/Audio — wir nutzen nur Text).
llama.cpp-Support inkl. Chat-Template-Updates seit April 2026.

**Gemma 3 bleibt** fuer Light-Tier (4-GB-Setups) und als Backward-
Compat-Pfad fuer User, die bei der kleineren Disk-Groesse bleiben
wollen.

Alle mit gepinten SHA-256-Hashes; Download ueber `download_llm()` in
`transcription/model_downloader.rs` mit in-flight Verifikation.
unsloth-Re-Packs werden bevorzugt, weil bartowski-/google-Original-Repos
ein Lizenz-Gate haben (Gemma-Akzept beim ersten Download).

**Bekannter Build-Quirk (automatisiert):** llama-cpp-sys-2 0.1.146's
build.rs hat einen TOC/TOU-Bug mit dangling Symlinks im `target/
debug/`. Wird vom `predev`/`prebuild`-Hook in `package.json`
(`scripts/clean-dangling-libs.mjs`) automatisch vor jedem Tauri-Build
geraeumt. Siehe PLATFORMS.md fuer Details.

**NVIDIA-Builder-Pfad (Phase 3b, Task #27 — opt-in):**
Builder mit CUDA-Toolkit + libvulkan-dev koennen ein Bundle mit
Runtime-Backend-Dispatch erzeugen:
```bash
cargo build --release --features embedded-cuda-dynamic
```
Das Feature aktiviert llama-cpp-2's `dynamic-backends`-Feature: GGML
baut CPU/Vulkan/CUDA als separate `ggml-*.so`-Files. Zur Laufzeit
laedt llama.cpp ueber `ggml_backend_load_all()` die verfuegbaren
Backends und nutzt das schnellste (typisch CUDA wenn `libcudart.so.13`
auf dem User-System ist, sonst Vulkan, sonst CPU).

**Wichtig zur Lizenz-Sauberkeit:** CUDA-Runtime wird **nicht**
gebundlet (NVIDIA-EULA-Konflikt mit GPL-3 — siehe Phase-3-Architektur-
Recherche). Der User muss CUDA-Toolkit oder mindestens die
`cuda-runtime`-Pakete selbst installiert haben. Ohne CUDA-Runtime
faellt der Pfad transparent auf Vulkan zurueck. Die `dynamic-backends`-
Build-Variante ist also ein Hybrid: NVIDIA-User mit CUDA-Treiber
bekommen die volle Speed, alle anderen werden via Vulkan bedient —
mit einem einzigen Bundle. Standard-Default-Build (ohne das Feature)
ist Vulkan-only und braucht kein CUDA-Toolkit auf der Build-Maschine.

### Ollama (lokal, kein BYOK-Key — Opt-in)

Embedded ist seit Mai 2026 der **Standardpfad** für `processing = "local"`.
Ollama bleibt als opt-in für User, die ihre bestehende Daemon-Installation
weiter nutzen wollen oder Modelle laufen, die noch nicht als GGUF im
Embedded-Pfad slot-basiert ausgewählt sind. Aktivierung pro Modus via
`local_engine = "ollama"`. Bestehende Phase-1/2-TOMLs ohne explizites
`local_engine` werden in `Mode::migrate_deprecated_fields` automatisch
auf `"ollama"` gesetzt (verifiziert über Tests in `core/modes.rs`).

- **Endpoint (Default):** `POST http://127.0.0.1:11434/api/chat`
- **Auth:** keine (lokaler HTTP-Server)
- **Default-Modell (ab Mai 2026):** `gemma3:4b` (vorher `qwen2.5:7b`)
  — Gemma 3 4B-IT von DeepMind, ~3 GB Footprint, 140+ Sprachen, sehr
  stark auf Deutsch. Wechsel über `Mode.ollama_model_tag` pro Modus
  (Deprecated-Alias `local_llm_model` wird beim Load auto-migriert).
- **Body:** Chat-Format analog zu OpenAI, plus Ollama-spezifische
  Felder:
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
- **`keep_alive`:** Duration-String, steuert wie lange Ollama das
  Modell nach dem Call im RAM/VRAM hält. Aus
  `Settings.ollama_keep_alive` (Default `"5m"`). `"0"` = sofortiges
  Unload (Memory-Pressure-Profil), `"-1"` = unbegrenzt warm halten.
- **`options.{temperature,top_p,repeat_penalty}`:** alle aus dem
  Mode-TOML; `None` = Ollama-Server-Default. Empfehlung für "faithful
  rewrite, do not extend": 0.2 / 0.8 / 1.05.
- **Response-Pfad:** `message.content`
- **Timeout:** 300 s (lokale Inferenz kann auf CPU dauern)
- **Endpoint überschreibbar:** Settings-Feld `ollama_url`.

## Secret-Handling

API-Keys sind pro Provider im File `~/.config/.../secrets.json` (chmod
0600) plus best-effort im OS-Keychain. **xAI ist ein Sonderfall:** ein
einziger Eintrag deckt STT *und* LLM ab, weil beide Endpoints denselben
Key nutzen.

Keys werden **niemals** geloggt (siehe CLAUDE.md §8) — Logging zeigt
nur die Key-Länge zur Diagnostik. Provider-Requests gehen
ausschließlich durch das Rust-Backend; der Key verlässt den Prozess
nicht ins Frontend (IPC `get_provider_status` liefert nur
`{ configured: bool, error: Option<String> }`).

## Bekannte Limitierungen

### xAI STT — keine Sprach-Erzwingung

xAI's STT-API akzeptiert keinen Parameter, mit dem sich die Sprach-
Erkennung festnageln ließe. Das `language`-Feld in der Request steuert
nur Text-Formatting (z.B. Schreibweise von Zahlen und Währungen),
nicht die akustische Spracherkennung. Die Erkennung ist serverseitig
hartcodiert auto-detect.

**Praktische Konsequenz:** Bei kurzen, sprachneutralen Diktaten
(z.B. einzelne Eigennamen, technische Begriffe, kurze Befehle) kann
das Modell daneben raten — z.B. einen deutschen Befehl als englisch
interpretieren und phonetisch transkribieren.

**Workaround:** Auf lokales Whisper-STT (`transcription = "local"` im
Modus) wechseln; dort lässt sich `language = "de"` erzwingen. Für
längere, klar deutschsprachige Diktate ist xAI in der Praxis robust,
deshalb akzeptieren wir das Limit für die Cloud-Modi und planen
**keinen** Fix (würde Provider-API-seitig erfordern und ist nicht in
unserer Hand).

**Quellen:** Verhalten beobachtet in Eigenpraxis (Stand Mai 2026); xAI
hat das Verhalten nicht offiziell als API-Constraint dokumentiert, also
ist eine spätere Änderung jederzeit möglich.

## Wenn du einen neuen Provider einbaust

1. Offizielle Provider-Doku via WebFetch ziehen — nicht auf diese
   Datei oder ähnlich-aussehende APIs verlassen.
2. Prüfen, ob er strukturell zu einem bestehenden Wrapper passt
   (`whisper_compatible.rs` für Whisper-API-kompatible STT,
   `openai_compatible.rs` für Chat-Completions-kompatible LLM).
   **Nur** dann in den Wrapper aufnehmen, wenn die Verwandtschaft real
   ist — sonst eigenständige Datei wie Deepgram / Anthropic.
3. Auth-Modus genau anschauen: Bearer ist üblich, aber Deepgram nutzt
   `Token`, Anthropic `x-api-key`. Pflicht-Header (`anthropic-version`)
   beachten.
4. Secret-Eintrag in `core/config.rs` ergänzen + Factory in
   `transcription/mod.rs` bzw. `processing/mod.rs` erweitern.
5. Tests für Response-Parsing (Mock-Response → `text`-Extraktion).
