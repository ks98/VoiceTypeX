# Architecture

> This file documents the most important structures and data flows.
> Detailed implementation decisions live as comments in the code, and the
> senior-engineer mindset and repo conventions live in [`CLAUDE.md`](../CLAUDE.md).

## Tech Stack

Fixed — alternatives are not introduced without prior discussion.

| Layer | Choice |
|---|---|
| App framework | Tauri 2 (stable) |
| Backend | Rust 2021+ |
| Frontend | React 18 + TypeScript + Vite |
| Styling | TailwindCSS + shadcn/ui |
| Frontend state | Zustand |
| Async runtime | tokio |
| Audio | cpal + hound (WAV) + rubato (sinc resampling) |
| Local STT | whisper-rs 0.16 + Silero-VAD v6, **default backend `gpu-vulkan`** (Phase 3a, May 2026); `gpu-cuda`/`gpu-metal`/`gpu-coreml` opt-in, `fast-cpu` (OpenBLAS) as the headless fallback. The CPU fallback when no Vulkan device is present is internal to whisper.cpp — not an app-code path. |
| Local LLM | **Embedded llama-cpp-2 0.1.146** with Vulkan + `dynamic-link` has been the production standard since May 2026 (no external daemon needed, GGUF runs inside the VoiceTypeX process) — **Linux/macOS only**. Per-mode switch via `local_engine = "embedded"` (default) vs `"ollama"`. Ollama remains opt-in for users who run their own daemon. **On Windows the embedded LLM is not compiled** (Issue #1: whisper-rs-sys + llama-cpp-sys-2 collide on duplicate ggml symbols during MSVC linking); there `local_engine` defaults to `"ollama"`, and the local LLM runs via a self-installed Ollama daemon or the cloud. |
| Cloud STT | xAI (one-shot REST), OpenAI Whisper, Groq Whisper, Deepgram |
| Cloud LLM | xAI Grok (default `grok-4-fast-non-reasoning`), OpenAI GPT, Anthropic Claude |
| HTTP client | reqwest (rustls-tls) |
| Config | TOML (`serde` + `toml`) |
| Logging | tracing + tracing-subscriber + ring buffer for the UI |
| Secrets | File (`~/.config/.../secrets.json`, chmod 0600) as the source of truth, OS keychain as a best-effort mirror |
| Audio cues | rodio |
| File watching | notify (mode hot-reload) |
| Repo & CI | GitHub (GitHub Actions) |

**Tauri plugins (used):** global-shortcut (X11/Windows), store,
dialog, notification, os, fs, clipboard-manager, autostart, updater
(auto-update for AppImage + Windows NSIS), process (relaunch after update).

**Tauri plugins (deliberately not used):** tauri-plugin-shell.

Wire-protocol details of the cloud providers (endpoints, auth headers,
multipart ordering, response parsing) are documented in
[`PROVIDERS.md`](PROVIDERS.md).

## State Machine

A single global state machine drives the pipeline. Implemented in
[`src-tauri/src/core/state.rs`](../src-tauri/src/core/state.rs):

```
Idle ─► Recording ─► Transcribing ─┬─► Postprocessing ─► Injecting ─► Idle
                                   └─► Injecting ─► Idle  (mode without LLM)
                                              ▲
                              Error ◄─ any stage (stays here)
                                │
                                └─► Idle  (recovery via menu hotkey)
```

`AppState::can_transition_to(...)` validates every transition. Invalid
transitions return `VoiceTypeError::InvalidStateTransition` and are
checked by a dedicated test.

**Errors stay visible:** A pipeline error moves the state to
`Error(msg)` and **stays there** — it does not immediately jump back to
`Idle`. Otherwise the following `Idle` would overwrite the `Error` frame
in the `watch` channel (coalescing) before the `app://state` emitter
sees it, and the overlay would never show an error. In the `Error`
state the tray icon turns red and the overlay shows the error text
(clickable → logs). The next menu hotkey clears `Error → Idle`
(recovery) and opens the menu — so `handle_menu_hotkey` treats `Error`
the same as `Idle`.

The `StateBus` uses `tokio::sync::watch` (the latest value is enough;
multiple subscribers, no backpressure issues). Subscribers: tray-icon
update, tray recording pulse, frontend event emitter (`app://state`),
overlay state listener.

`start_recording` additionally emits `app://active-engine` — the
`EngineStatus` from `core::modes::resolve_engine_status` — so the overlay's
status line shows the active mode's STT/LLM engine + model (local vs cloud).
(#8)

## Pipeline (menu hotkey + toggle)

[`src-tauri/src/pipeline/mod.rs`](../src-tauri/src/pipeline/mod.rs)
orchestrates the stages. There is **one** global hotkey
(`Settings.menu_hotkey`) that triggers different behavior depending on
the pipeline state:

```
Hotkey-Press
   │
   ▼
handle_menu_hotkey(app, ctx)
   │
   ├─ State == Idle ─────────────► menu.show() + menu.set_focus()
   │                                Frontend shows the mode list
   │                                User: ↑/↓, Enter, Esc
   │                                Enter → invoke("start_recording", {modeId})
   │
   ├─ State == Recording ───────► finish_recording_and_inject(active_mode)
   │
   └─ else ──────────────────────► ignored (pipeline busy)

invoke("start_recording", { mode_id })
   │
   ▼
execute_mode(app, ctx, mode)   ─── State == Idle ? start : (Toggle-Stop)
   │
   ▼
start_recording(app, ctx, mode)
   │  • ctx.active_mode = Some(mode)   ── so the stop hotkey knows which mode is running
   │  • State → Recording
   │  • overlay.show()                 (Wayland focus theft is neutralized in finish())
   │  • Start cue (short beep)
   │  • RecorderHandle::start()        (cpal stream in its own thread)
   │
Hotkey press in the Recording state
   │
   ▼
finish_recording_and_inject(app, ctx, active_mode)
   │  • ctx.active_mode = None
   │  • State → Transcribing, Stop-Cue
   │  • recorder.stop_and_finalize() → 16 kHz mono f32 samples
   │  • Local STT: LocalTranscriber::transcribe_samples (f32 straight in)
   │    Cloud STT: encode_wav_16k_mono → Transcriber::transcribe_oneshot
   │  • If processing != none: State → Postprocessing,
   │    Processor::process (Ollama or cloud LLM)
   │  • State → Injecting
   │  • overlay.hide() + 80 ms pause   (focus jumps back to the target app)
   │  • Injector::inject               (libei on Wayland, enigo+clipboard otherwise)
   │  • State → Idle
```

`AppContext.active_mode: Arc<Mutex<Option<Mode>>>` is the link between
start (knows the mode from the IPC call) and stop (the menu hotkey does
not itself know which mode is running). It is set in `start_recording`
and cleared at the very top of `finish_recording_and_inject`.

UI trigger buttons in the mode list call `execute_mode` directly — the
toggle logic is the same as for the hotkey.

## Edit Modes (selection → LLM → replace/insert)

Modes with `Mode.input == Selection` transform selected text instead of
producing a new dictation. The pipeline changes only minimally for this
— no new state:

1. **Eager capture:** in the `Idle` branch, `handle_menu_hotkey` reads
   the selection *before* `menu.show()` (the menu steals focus, but
   reading needs the focused target app). Gated on the presence of an
   edit mode (`capture_selection_if_edit_modes`) — pure dictation setups
   pay nothing. The result lands in `AppContext.selection_buffer`.
2. **Reading** happens via `TextInjector::read_selection()`. On Linux
   (X11 **and** Wayland) the **PRIMARY selection** is read directly
   (`injection::read_primary_selection_linux` via arboard; X11 natively,
   Wayland via wlr/ext-data-control) — focus-independent, without
   Ctrl+C, without touching the CLIPBOARD clipboard. On Windows (no
   PRIMARY selection) `ClipboardFallbackInjector` simulates Ctrl+C and
   reads the clipboard (save → copy → read → restore).
3. **Composition** (`core::edit::compose_edit_input`): in
   `finish_recording_and_inject`, when `input == Selection`, the
   selection + the transcribed dictation are framed into *one* user
   string (`<selected_text>…</selected_text>\n<instruction>…</instruction>`)
   and handed to the `Processor` as the "transcript" — the `Processor`
   trait stays unchanged.
4. **Output resolution** (`core::edit::resolve_output_action`): after
   the LLM, `Mode.output` is applied. With `Auto` the sentinel
   (`@@REPLACE`/`@@APPEND`/`@@PREPEND` on line 1) parses the action,
   otherwise `Mode.output_fallback` takes over. The action flows into
   the injection as `InjectOptions.action`; `Append`/`Prepend` make the
   injector collapse the selection (arrow key) before pasting.

Platform reach and known limits: [`PLATFORMS.md`](PLATFORMS.md).

## Trait Layers

Provider- and inject-critical functionality is abstracted behind traits.
Platform selection happens at runtime (Linux uses `WAYLAND_DISPLAY` to
differentiate in `core::session::is_wayland()`).

| Trait | File | Implementations |
|---|---|---|
| `Transcriber` | `transcription/mod.rs` | `LocalTranscriber` (whisper-rs), `XaiTranscriber`, `OpenAITranscriber`, `GroqTranscriber`, `DeepgramTranscriber` |
| `Processor` | `processing/mod.rs` | `LlamaEmbeddedProcessor` (embedded llama-cpp-2, **default engine**), `OllamaProcessor` (local Ollama daemon, opt-in), `XaiProcessor`/`OpenAIProcessor` (via the shared `OpenAICompatibleClient`), `AnthropicProcessor` |
| `TextInjector` | `injection/mod.rs` | `ClipboardFallbackInjector` (X11/Windows: enigo Ctrl+V), `WaylandLibeiInjector` (Wayland: libei via xdg-desktop-portal.RemoteDesktop) — the trait additionally carries `read_selection()` (the input side of the edit modes, see below). On KDE Plasma 6 the paste shortcut (Ctrl+Shift+V for terminals vs Ctrl+V) is chosen via `injection/focus_tracker.rs` — a bundled KWin script reports the active window's `resourceClass` over a zbus service, cached in `AppContext.kde_focus` |

**Hotkey registration** is platform-direct (no trait, see
[CLAUDE.md](../CLAUDE.md)) and registers **exactly one** global
shortcut (`Settings.menu_hotkey`):

- X11/Windows: `pipeline::register_menu_hotkey()` registers it directly
  via `app.global_shortcut().on_shortcut()` from
  `tauri-plugin-global-shortcut`. Only `ShortcutState::Pressed` is
  processed — release events are irrelevant (no more PTT).
- Wayland: `pipeline::spawn_wayland_hotkey_session()` runs a long-lived
  portal session via
  `hotkey::linux_wayland::run_global_shortcuts_session()` with a single
  `WaylandShortcutSpec` entry (`id = "open_menu"`). Events flow back as
  `HotkeyEvent` over a broadcast channel; released events are filtered
  in the dispatcher.

## AppContext (Tauri State Singleton)

[`core/app_context.rs`](../src-tauri/src/core/app_context.rs):

```rust
pub struct AppContext {
    pub state_bus: StateBus,
    pub modes: Arc<ModesRegistry>,
    pub recorder_slot: Arc<Mutex<Option<RecorderHandle>>>,
    pub active_mode: Arc<Mutex<Option<Mode>>>,         // which mode is currently running?
    pub effective_menu_hotkey: Arc<RwLock<Option<String>>>, // Wayland: trigger returned by the portal
    pub transcriber: Arc<dyn Transcriber>,             // app-default transcriber (local or cloud)
    pub local_transcriber: Arc<LocalTranscriber>,      // concrete type for the streaming worker (Phase 2)
    pub local_llm_processor: Arc<LlamaEmbeddedProcessor>, // embedded-LLM default processor (Phase 3b) — #[cfg(not(windows))]
    pub extra_transcribers: Arc<Mutex<HashMap<String, Arc<LocalTranscriber>>>>, // per-mode Whisper slot cache
    pub extra_llm_processors: Arc<Mutex<HashMap<String, Arc<LlamaEmbeddedProcessor>>>>, // per-mode LLM slot cache — #[cfg(not(windows))]
    pub active_streaming_handle: Arc<Mutex<Option<JoinHandle<()>>>>, // Phase-2 streaming worker handle
    pub injector: Arc<dyn TextInjector>,
    pub selection_buffer: Arc<Mutex<Option<String>>>,  // eager-captured selection for edit modes
    pub settings: Arc<RwLock<Settings>>,
    pub settings_path: PathBuf,
    pub log_buffer: LogRingBuffer,
    pub model_dir: PathBuf,
    pub modes_dir: PathBuf,
}
```

Made available as a Tauri singleton via `app.manage(Arc<AppContext>)`;
every IPC command pulls it out via
`tauri::State<'_, Arc<AppContext>>`.

## Mode Hot-Reload

`ModesRegistry` (`core/modes.rs`) holds the mode list in
`Arc<RwLock<Vec<Mode>>>`. A `notify::RecommendedWatcher` runs in its own
thread and reacts to `*.toml` changes in `app_config_dir/modes/`. On
every change the whole list is re-read and subscribers are notified via
`broadcast::Sender<ModesEvent>`.

On first launch,
[`core/default_modes.rs`](../src-tauri/src/core/default_modes.rs) copies
the 9 default TOMLs embedded in the binary into
`app_config_dir/modes/`.

## Audio Pipeline

`cpal::Stream` is `!Send`. Solution: a dedicated thread holds the
stream, and a send-able handle (`RecorderHandle`) communicates with the
worker over a channel. Audio is collected as f32; on stop
`stop_and_finalize` runs:

1. Stereo→mono mix (mean per frame)
2. `rubato::SincFixedIn` resampling to 16 kHz

It returns the **16 kHz mono f32 samples**, not a WAV. The split per STT
target (issue #46):

- **Local** — the f32 buffer goes straight to
  `LocalTranscriber::transcribe_samples`; whisper-rs consumes f32
  natively, so there is no encode/decode and no s16 quantization.
- **Cloud** — `encode_wav_16k_mono` lazily wraps the f32 in a `hound`
  WAV (PCM s16 LE) for the multipart upload, then
  `Transcriber::transcribe_oneshot` runs. The trait entry on the local
  transcriber stays (decode WAV → delegate to `transcribe_samples`) for
  parity, but the pipeline's local path does not use it.

## Local STT Pipeline (Phase 1, May 2026)

[`transcription/local.rs`](../src-tauri/src/transcription/local.rs) and
[`transcription/model_downloader.rs`](../src-tauri/src/transcription/model_downloader.rs):

**Whisper sampling (default):**
- `SamplingStrategy::BeamSearch { beam_size: 2, patience: 1.0 }` —
  whisper.cpp runs `beam_size` decoders in parallel, so decode cost is
  ~linear in the width; on short dictation beam>2-3 buys <2 % WER for a
  large latency hit, so the default is 2 (lowered from 5). Per-mode /
  settings override stays available for users who want max accuracy.
- `suppress_blank=true`, `no_speech_thold=0.6`, `temperature=0.0` with
  `temperature_inc=0.2` as the fallback when `logprob_thold` trips.

**Silero-VAD v6.2.0:**
- Path: `app_config_dir/models/ggml-silero-v6.2.0.bin` (~885 kB).
- Pulled in best-effort alongside the first Whisper model download — if
  the file is missing, Whisper runs transparently without VAD and logs a
  WARN line per call.
- Defaults deliberately more conservative than upstream:
  `min_silence_duration_ms=500` (vs. 100), `speech_pad_ms=200`
  (vs. 30) — no mid-sentence cuts, no consonant clipping.

**Model slots** (`ModelSlot::from_setting`):

| Slot string | File | Size | Source |
|---|---|---|---|
| `large-v3-turbo-q8_0` *(Default)* | `ggml-large-v3-turbo-q8_0.bin` | ~874 MB | ggerganov/whisper.cpp |
| `large-v3-turbo-german-q5_0` | `ggml-model-q5_0.bin` | ~574 MB | cstr/whisper-large-v3-turbo-german-ggml (primeline fine-tune, Apache 2.0) |
| `large-v3-turbo-german-q8_0` | `ggml-large-v3-turbo-german-q8_0.bin` | ~874 MB | Pomni/whisper-large-v3-turbo-german-ggml-allquants (same primeline fine-tune, Apache 2.0; Vulkan-safe Q8) |
| `large-v3-turbo-q5_0` | `ggml-large-v3-turbo-q5_0.bin` | ~547 MB | ggerganov/whisper.cpp |
| `small-q5_1` | `ggml-small-q5_1.bin` | ~190 MB | ggerganov/whisper.cpp |
| `large-v3-turbo` | `ggml-large-v3-turbo.bin` | ~1624 MB | ggerganov/whisper.cpp (F16) |

The picker in Settings / the ModeEditor renders these as comparison cards
(`src/components/WhisperModelCards.tsx`, data in `src/lib/whisperModels.ts`)
with qualitative Speed/Accuracy bars, a DE badge on the German fine-tunes,
the RAM footprint, and a hardware- + language-aware "recommended" marker
(`recommendWhisperSlot`).

All slots have pinned SHA-256 hashes; a mismatch triggers a re-download
and never accepts a corrupted file. The hashes come from the HF Git-LFS
pointers (`curl .../raw/main/<file> | head -3`).

**Bootstrap order** (`lib.rs`): settings are loaded before the pipeline
is constructed, and the `LocalTranscriber` builds its model path from
`settings.whisper_model_path` (taking precedence) or
`ModelSlot::from_setting(&settings.whisper_default_slot).filename()`.
Previously the path was hard-coded; user settings were ignored.

### Streaming (Phase 2)

When `mode.transcription = "local"`, `start_recording` spawns a parallel
streaming-decode worker. It runs during the `Recording` state in
parallel with the recording and feeds the overlay a live-updating
partial text, so the user can already see what Whisper understands while
speaking.

```
[Hold-to-record]──────────────────────────────[Release]
[Recorder cpal buffer grows continuously]
[Streaming worker]
   ├─ t+2.0s: Snapshot → Convert 16k Mono → transcribe_streaming_pass → "Heute"
   ├─ t+2.8s: Snapshot → ...                                          → "Heute scheint"
   ├─ each new decode → emit app://partial-transcript
   │  (LocalAgreement-2 still computes prefix convergence as telemetry)
   └─ Overlay shows "Heute scheint" under "Listening ..."
                                                       [Release]
                                                       └─►abort()
                                                       └─►final pass (BeamSearch=2, audio_ctx)
                                                          overwrites everything
```

**Gating**: the worker spawns only when `mode.transcription ==
TranscriptionTarget::Local`. Cloud modes still run one-shot after the
stop hotkey — their REST endpoints (xAI/OpenAI/Groq/Deepgram) have no
comparable streaming interface.

**Decode profile**: streaming passes use `DecodeProfile::Streaming`:
greedy sampling instead of BeamSearch (3× faster) plus `set_audio_ctx`
with a dynamically computed frame count (`dynamic_audio_ctx_frames`) for
short audio. From ~25 s on the trick is dropped (returns `None`) so the
mel encoder cuts nothing off. The final pass after stop uses
`DecodeProfile::Final` (BeamSearch); it now applies the **same**
`audio_ctx` shortening on short clips (perf #1 — the final pass
previously always ran the full 30 s mel encoder, the dominant cost of the
felt paste latency) and delivers the definitive text, overwriting any
partial output.

**LocalAgreement-2** (`transcription/local_agreement.rs`, Machacek et
al. arXiv 2307.14743): computes the stable word prefix from two
consecutive decodes. Tokenization on whitespace, with punctuation
staying attached to its word.

**Currently telemetry only, no emit gate.** The original plan was to
emit only the stable prefix to the overlay; in practice a streaming pass
on CPU-only hardware takes 8–12 s, so a second decode often does not
complete before the stop hotkey, which would have made LA-2 block all
emits. Pragmatically, therefore, every new non-empty decode is emitted
directly (text can "shimmer" if a later pass revises an earlier one);
the final pass after stop overwrites it authoritatively anyway. The LA-2
convergence is still logged as `tracing::debug!` telemetry, to observe
hardware performance across sessions.

**Abort and state transition**: `finish_recording_and_inject` calls
`JoinHandle::abort()` on the streaming handle before the final pass
runs. That interrupts the loop at the next `.await`; CPU work inside
`spawn_blocking` still finishes but does not block the pipeline. Right
after `abort()`, an empty `app://partial-transcript` event is sent,
which clears the overlay partial. The state machine itself does not
change from Phase 1 — streaming is a parallel helper loop, not a new
state.

**Configuration** (all constants in `pipeline/mod.rs`):

| Constant | Value | Effect |
|---|---|---|
| `STREAMING_INITIAL_WAIT_MS` | 2000 | Wait before the first decode so the buffer has substance |
| `STREAMING_INTERVAL_MS` | 800 | Decode frequency |
| `STREAMING_MIN_AUDIO_SAMPLES` | 16000 | Decode only from 1.0 s of audio (16 kHz) — shorter audio lands in whisper.cpp's "single timestamp ending" skip path and yields empty outputs |

## Local LLM — Two Paths as of Phase 3b

`Mode.local_engine` selects between the two paths per mode:

### Embedded (`local_engine = "embedded"`) — **default engine as of May 2026**

[`processing/embedded.rs`](../src-tauri/src/processing/embedded.rs):

- llama-cpp-2 0.1.146 with features `vulkan + sampler + dynamic-link`.
  `dynamic-link` is mandatory — otherwise the statically linked ggml
  versions of whisper-rs-sys and llama-cpp-sys-2 collide.
- `LlamaBackend::init()` once via a `OnceLock` singleton.
- Model cache behind `Arc<RwLock<Option<LlamaModel>>>` (analogous to
  `LocalTranscriber`).
- Per `process()` call: a fresh `LlamaContext` + `LlamaBatch`, chat
  template from the GGUF (`model.chat_template(None)`), sampling chain
  `penalties → top_p → temp → dist` (or `greedy` when
  temperature == 0).
- Model path resolution: `Settings.llm_model_path` (override) →
  `LlmModelSlot::from_setting(Settings.llm_default_slot).filename()`
  under `app_config_dir/models/`.
- AppContext: `local_llm_processor: Arc<LlamaEmbeddedProcessor>`,
  constructed at startup, with the model loaded LAZILY.
- **Per-mode slot override** via `Mode.embedded_llm_slot`: points to an
  alternative GGUF slot. The resolver in `pipeline/mod.rs` compares
  against `Settings.llm_default_slot` — on a match (or `null`) the
  global processor is reused, otherwise a new instance is cached lazily
  in `AppContext.extra_llm_processors` (HashMap, slot slug → Arc).
  Analogously there is `Mode.whisper_model_slot` +
  `AppContext.extra_transcribers` for Whisper overrides.

### Ollama (`local_engine = "ollama"`) — opt-in for external daemon use

[`processing/local.rs`](../src-tauri/src/processing/local.rs):

- Default model recommendation for Ollama: `gemma3:4b` (DeepMind, Mar
  2025) — 140+ languages, ~3 GB footprint, sweet spot for 8–16 GB
  devices. Previously `qwen2.5:7b`.
- **Per-mode sampling** via `ProcessOpts.{temperature, top_p,
  repeat_penalty}`, filled from `Mode.temperature/top_p/repeat_penalty`
  (TOML fields). Default for "faithful rewrite, do not extend":
  0.2 / 0.8 / 1.05.
- **`keep_alive` per request** from `Settings.ollama_keep_alive`
  (default `"5m"`, `"0"` for immediate unload on memory-pressure
  profiles, `"-1"` for keeping it warm indefinitely).
- Cloud processors (xAI/OpenAI/Anthropic) get the same sampling fields
  passed through, to the extent the provider respects them.

### Branching in the Pipeline Code

`pipeline/mod.rs::run_local_processing` looks at `mode.local_engine`:
`"embedded"` (or `None` — default) → `resolve_embedded_llm(ctx, mode)`
(global processor or cache lookup via `mode.embedded_llm_slot`),
`"ollama"` → `run_local_processing_ollama` (Ollama HTTP call setup with
keep_alive + `mode.ollama_model_tag`, falling back to the deprecated
`local_llm_model`). An unknown engine value → `Mode` error message.
Sampling fields are passed through into both paths via
`ProcessOpts.{temperature, top_p, repeat_penalty, max_tokens}`.

**Windows:** `resolve_embedded_llm` and the `"embedded"` arm are gated
on `#[cfg(not(target_os = "windows"))]`; the default engine value there
is `"ollama"` instead of `"embedded"`. If a mode nonetheless explicitly
triggers `local_engine = "embedded"` (e.g. the bundled *Correcting
Dictation*, whose TOML is identical across platforms), the Windows arm
returns a clear control error message ("embedded LLM not available on
Windows — use Ollama or a cloud provider") instead of panicking. The
TOMLs stay byte-identical across all platforms, and `Mode::validate`
accepts `"embedded"` platform-agnostically (only the runtime path is
gated).

**Existing user TOMLs from Phase 1/2** (with `local_llm_model` or
`ollama_model_tag` but no `local_engine`) are explicitly set to
`local_engine = "ollama"` on load in
`Mode::migrate_deprecated_fields` — otherwise the default switch from
`"ollama"` to `"embedded"` (May 2026) would reroute these modes onto the
wrong engine path and fail with "GGUF slot not found".

## Wayland Auto-Paste

[`injection/linux_wayland.rs`](../src-tauri/src/injection/linux_wayland.rs)
+ [`injection/libei_worker.rs`](../src-tauri/src/injection/libei_worker.rs):

```
inject(text)
   │
   ▼
clipboard.write_text(text)        (wl_data_device.set_selection)
   │
   ▼
ensure_session()                   (lazy on the first inject)
   │  • load restore_token from ~/.config/.../wayland_session.json (if present)
   │  • ashpd::RemoteDesktop::create_session → select_devices(KEYBOARD, prior_token)
   │  • start(...) → first time: permission dialog; with a valid token: silent
   │  • connect_to_eis(...) → EIS file descriptor
   │  • new restore_token is written to disk
   │  • spawn worker thread with the FD + mpsc<KeyCommand>
   │       └─ EI handshake: HandshakeVersion → Connection → Seat → Device → Keyboard
   │       └─ on Device::Resumed: start_emulating(seq, serial)
   │       └─ ready signal to the tokio side via oneshot
   ▼
60 ms sleep                        (compositor roundtrip for set_selection)
   │
cmd_tx.send(KeyCommand::CtrlV)    (to the libei worker)
   │
Worker: 4 frames with per-frame flush + 1 ms pause:
   ▼
   keyboard.key(LEFTCTRL, Press) + frame + flush + sleep
   keyboard.key(V, Press)        + frame + flush + sleep
   keyboard.key(V, Released)     + frame + flush + sleep
   keyboard.key(LEFTCTRL, Released) + frame + flush
   ▼
80 ms sleep                        (libei processing time)
```

Mandatory disciplines from the libei spec (see `protocol.xml` and the
code comments in `libei_worker.rs`):
- `start_emulating` exclusively in the `Resumed` handler (otherwise
  silent discard)
- per `frame`, only one `key` state change per key
- `frame.time` strictly monotonic, CLOCK_MONOTONIC-based (Rust
  `Instant::now()`)
- `sequence` counter monotonic over the app's lifetime
- a `Paused` event resets `emulation_active`; the next `Resumed` starts
  a new emulation sequence with an incremented `sequence`

## Windows

| Window | Purpose | Size | Focus | Pointer events | Position |
|---|---|---|---|---|---|
| `main` | Main window (Settings, Modes, Logs) | 960 × 720, resizable | yes | yes | centered |
| `overlay` | Status indicator during Recording / Transcribing / … | 520 × 96, **non-resizable** | **no** (`focus: false`) | **none** (CSS) | centered |
| `menu` | Mode selection via arrow navigation + Enter | 480 × 360, non-resizable, scrollable with many modes | yes | yes | centered |

All three windows load the same `index.html`; routing happens in
`src/main.tsx` via the `?window=overlay` / `?window=menu` URL query,
otherwise it falls back to `App.tsx` (main window).

### Visibility Is Backend-Driven

No frontend show/hide — the backend orchestrates both secondary windows
in `pipeline/mod.rs` and `ipc/recording.rs`:

| Trigger | overlay | menu |
|---|---|---|
| `handle_menu_hotkey` (Idle) | — | `show()` + `set_focus()` |
| `start_recording` | `show()` | `hide()` (idempotent) |
| `finish_recording_and_inject` | `hide()` + 80 ms before libei inject | — |
| `spawn_overlay_state_listener` on `Error` | `show()` (error visible) | — |
| `spawn_overlay_state_listener` on `Idle` | `hide()` | — |
| IPC `cancel_menu` (Esc) | — | `hide()` |

### Overlay View (`src/views/Overlay.tsx`)

Lean: subscribes to `app://state`, renders phase-appropriate status text
(*"Listening …"*, *"Transcribing …"*, *"Processing …"*, *"Inserting …"*,
*"Error"*). No keyboard interaction, no pointer events (CSS protection,
in case the window ever stays visible).

### Menu View (`src/views/Menu.tsx`)

Reads modes from `useModesStore`, with the cursor initially on
`Settings.last_selected_mode_id` (one keypress for the most common
action). Keyboard handler on the root div:
- `↑` / `↓` / `Home` / `End` → cursor
- `Enter` → `invoke("start_recording", { modeId })` (the backend hides
  the menu window and shows the overlay)
- `Esc` → `invoke("cancel_menu")` (the backend hides the menu window)

### Wayland Focus Quirk

`menu.set_focus()` is not guaranteed to be honored on Wayland
compositors — the compositor decides. On KDE Plasma 6 it works reliably
because the menu window is focusable by default (Tauri windows accept
focus unless `focus: false` is set, as the overlay does), so
`menu.set_focus()` is honored. On wlroots compositors like Hyprland /
Sway focus can still fail to arrive; there the app is in
clipboard-fallback mode anyway
([`docs/PLATFORMS.md`](PLATFORMS.md) → *Hyprland / Sway / wlroots*).

### Wayland Hotkey Read-Back

On Wayland, `Settings.menu_hotkey` is only a **suggestion** to
`xdg-desktop-portal.GlobalShortcuts`. After the first `bind_shortcuts`,
KDE remembers the assignment and ignores later `preferred_trigger`
values; the user can additionally adjust the hotkey in
*System Settings → Global Shortcuts → VoiceTypeX*.

`hotkey::linux_wayland::run_global_shortcuts_session` therefore calls
`list_shortcuts` once after `bind_shortcuts` and writes the
`trigger_description` of the first action into
`AppContext.effective_menu_hotkey: Arc<RwLock<Option<String>>>`. The IPC
command `get_effective_menu_hotkey` reads this cache; the frontend
(`Settings.tsx → MenuHotkeyField`) shows a read-only field with the
effective trigger + a hint about the system settings on Wayland, while
on X11 / Windows the field stays editable.

## Persistence

| What | Where | Format |
|---|---|---|
| User settings (PTT, model slot, audio device, …) | `~/.config/.../settings.json` | JSON, chmod 0644 |
| API keys (BYOK) | `~/.config/.../secrets.json` (source of truth) + OS keychain (mirror) | JSON, chmod 0600 |
| Wayland `restore_token` | `~/.config/.../wayland_session.json` | JSON, chmod 0600 |
| Modes (hot-reload) | `~/.config/.../modes/*.toml` | TOML |
| Whisper models | `~/.config/.../models/*.bin` | GGML, SHA-256-verified |
| Silero-VAD | `~/.config/.../models/ggml-silero-v6.2.0.bin` | GGML, SHA-256-verified |
| GGUF LLM models (Phase 3b) | `~/.config/.../models/*.gguf` | GGUF, SHA-256-verified |

Settings + token are read at app start and written after every mutation
IPC (see `Settings::load_or_default` / `Settings::save` and
`WaylandLibeiInjector::ensure_session`).

## Logging

Tracing stack with four layers:
- `EnvFilter` (RUST_LOG compatible; default `voicetypex=info,tauri=info,warn`)
- `fmt::layer()` for stdout (dev)
- `LogRingBuffer::layer()` (in-memory, 500 lines, polled by the Logs view)
- a rolling on-disk file (`tracing-appender`, daily rotation, last 7 files
  kept) under the per-OS app log dir (`app_log_dir()`:
  `~/.local/share/<identifier>/logs` on Linux,
  `<LocalAppData>/<identifier>/logs` on Windows,
  `~/Library/Logs/<identifier>` on macOS) — so crashes survive a restart

`init_tracing` runs in `run()` before the Tauri app handle exists, so it
installs the first three layers plus an empty reloadable slot for the file
layer; the `.setup()` hook then resolves `app_log_dir()` and swaps the real
rolling-file layer in. Trade-off: events emitted before `.setup()` (the
active-backend line, Tauri plugin init) reach stdout + the ring buffer but
not the file. A failure to open the log dir is non-fatal (warn + continue).

CLAUDE.md's privacy/logging rules are strict: audio/transcript/LLM-response data **never** go
into the default logging. A diagnostic-logging toggle in the settings
would additively enable further calls, not filter existing ones.

## Frontend

React 18 + TypeScript strict + Tailwind v3 + Zustand.

- **Views (`src/views/`):** Settings, Modes, Logs, Overlay, Menu (Menu
  and Overlay are their own Tauri windows from the same `index.html`,
  distinguished via the `?window=` query in `main.tsx`).
- **Components (`src/components/`):** Sidebar, ThemeToggle, Field,
  OnboardingWizard, ModeEditor, TestTranscriptionSection,
  AutoPasteTestSection, ApiKeysSection
- **Stores (`src/store/index.ts`):** UI (tab state + theme choice),
  Settings, Modes — with async actions, one per IPC command
- **IPC wrapper (`src/lib/tauri.ts`):** the only place that uses
  `invoke()` directly; all commands exported by name

### Design Tokens & Theme

- **Tokens live as CSS custom properties** (RGB triplets) in
  `src/styles/globals.css` under `:root` (light) and `html.dark`
  (dark). Tailwind maps them to semantic classes in
  `tailwind.config.ts`: `bg-canvas/surface/elevated`,
  `text-fg/muted/faint`, `border-outline/strong`, `brand/brand-hover`,
  `status-*`.
- **Theme choice** (system/light/dark) lives in `src/lib/theme.ts`, is
  persisted in localStorage, and applied synchronously before the React
  render in `main.tsx` (FOUC prevention). A matchMedia listener reacts
  to OS theme changes when the user choice is "system".
- **Floating windows** (Menu, Overlay) follow the system theme instead
  of the app setting: separate Tauri renderers have their own
  localStorage and fall back to "system". A deliberate design decision
  — ambient notifications integrate into the desktop.

### Logo & Icons

- **Source of truth**: SVG files in `src-tauri/icons/source/`. The mark
  is Wave-to-Caret (4 audio bars → I-beam cursor) in brand indigo
  (`#3D5AFE`). Tray states consist of 7 SVGs (logo +
  idle/recording/recording_pulse/processing/done/error).
- **Render pipeline** (to be run manually on icon update):
  - `rsvg-convert -w N -h N source/logo.svg -o icon.png` for the bundle
    PNGs (32/128/256/512).
  - `magick PNG-Frames icon.ico` for the `.ico` with 16/32/48/256.
  - `rsvg-convert -w 64 -h 64 source/tray-X.svg -o tray/X.png` for each
    of the six tray states.
- **Web counterpart**: `src/components/Logo.tsx` is a React component
  with identical geometry and `fill="currentColor"` — theme-aware via
  Tailwind, embedded in the sidebar header and the
  OnboardingWizard step-1 hero. When adjusting the geometry, keep both
  places (SVG source + Logo.tsx) in sync.

## Internationalization (i18n)

A small custom hook layer instead of i18next — ~70 LOC, no runtime
dependency beyond the native `Intl.*` API. Target languages for
Release 1: `de`, `en`, `fr`, `es`, `it`. English is the source of truth
(`src/i18n/locales/en.json`).

**Data flow:**

```
tauri_plugin_os::locale()        Settings.locale            useI18nStore
 (Backend, first run)     ──►   (persisted, JSON)     ──►   (Frontend, Zustand)
                                       │                          │
                                       ▼                          ▼
                            pickSupported(raw)              useT() / useLocale()
                            ↓                                     │
                            "de"|"en"|"fr"|"es"|"it"              ▼
                                                            React-Components
```

- **The backend** does the first-run detection in the `setup` hook
  ([`lib.rs::run`](../src-tauri/src/lib.rs)) and persists the raw OS
  locale string in `Settings.locale`. A single-writer pattern, so the
  three webviews (main, overlay, menu) do not race. A deserialize
  validator filters hand-edits in the settings file (BCP-47-like ASCII
  form, max. 35 characters).
- **The frontend bootstrap** in [`main.tsx`](../src/main.tsx) fetches
  `Settings.locale` via IPC, maps it via `pickSupported()` to one of the
  supported languages (region suffix ignored, fallback `en`), and sets
  the `useI18nStore` *before* the React render.
- **The `useT()` hook** ([`src/i18n/index.ts`](../src/i18n/index.ts)) is
  stable via `useCallback([locale])`. Eager-loaded dictionaries
  (5 locales × ~5 KB), fallback chain `current → en → key`.
- **Plural rules** come from `Intl.PluralRules` — convention `key.one`,
  `key.other` (CLDR forms). Numeric `params.count` values automatically
  trigger plural selection.
- **The build gate** [`scripts/i18n-check.mjs`](../scripts/i18n-check.mjs)
  runs as `prebuild` and in the standalone target `pnpm i18n:check`:
  checks locale parity against `en.json`, scans `t("...")` calls for
  existence (including plural base forms), and validates template-literal
  prefixes (`t(\`app.tabs.${id}\`)` must match at least one key in
  `en.json`).

**Cross-window sync:** Each webview window (main, overlay, menu) has its
own `useI18nStore`. The language switcher in Settings calls three
things:
1. `ipcSetSettings({...locale})` — persists to `Settings.locale`.
2. `useI18nStore.setState({locale})` — immediate UI update in its own
   window.
3. `emit("i18n://locale-changed", {locale})` — a Tauri event that the
   other windows receive in [`src/main.tsx`](../src/main.tsx) via a
   `listen()` subscriber and use to update their store.

**What does NOT migrate live on a locale switch** (stays unchanged until
the app restarts):
- **The tray menu** (`Open settings`/`Quit` etc.) — Tauri 2 has no live
  swap for MenuItems; the backend reads the locale once in the `setup`
  hook and builds the menu from it.
- **Default modes** (name, description, `system_prompt`) — they are
  copied once into `app_config_dir/modes/` on first run and are user
  content from then on. A locale switch afterwards falls back to "the
  user edits or deletes and re-bootstraps manually".
- **Backend error messages** in banners — the strings come out of the
  backend in English (Phase 4 normalized them to plain English instead
  of the previously ASCII-transliterated German). A full error-code
  internationalization (a structured `UserError` enum + frontend
  mapping) is earmarked as a later refactor — users see the English
  strings independently of the UI locale for now.

**Required when adding new UI strings:**
1. Add the key to `en.json` (source of truth).
2. Add translations to `de.json`, `fr.json`, `es.json`, `it.json` in
   parallel (otherwise `pnpm i18n:check` reports missing keys).
3. Wire it up in the React code via `t("namespace.key")` or
   `t(\`namespace.${dynamic}\`)` (template prefixes are validated by the
   build gate against the current set of keys).

## Hardware Detection

[`core/hardware.rs`](../src-tauri/src/core/hardware.rs) detects the
available Whisper backends (CPU, OpenBLAS, Vulkan, CUDA, Metal, CoreML)
at startup via library probing. The recommendation is shown in the
settings UI; the user can choose the Cargo feature at build time — the
runtime code uses whatever the given build provides.

## Open Items

None currently. macOS is not in scope; the
`#[cfg(target_os = "macos")]` stubs remain in place so the code compiles
on macOS, but they are not an active implementation target.
