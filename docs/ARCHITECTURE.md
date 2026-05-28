# Architektur

> Diese Datei dokumentiert die wichtigsten Strukturen und Datenflüsse.
> Detaillierte Implementierungs-Entscheidungen stehen als Kommentare im Code,
> Senior-Engineer-Mindset und Repo-Konventionen in [`CLAUDE.md`](../CLAUDE.md).

## Tech-Stack

Festgelegt — Alternativen werden nicht ohne Rücksprache eingeführt.

| Schicht | Wahl |
|---|---|
| App-Framework | Tauri 2 (stabil) |
| Backend | Rust 2021+ |
| Frontend | React 18 + TypeScript + Vite |
| Styling | TailwindCSS + shadcn/ui |
| Frontend-State | Zustand |
| Async-Runtime | tokio |
| Audio | cpal + hound (WAV) + rubato (Sinc-Resampling) |
| Lokales STT | whisper-rs 0.16 + Silero-VAD v6, **Default-Backend `gpu-vulkan`** (Phase 3a, Mai 2026); `gpu-cuda`/`gpu-metal`/`gpu-coreml` opt-in, `fast-cpu` (OpenBLAS) als Headless-Fallback. CPU-Fallback bei fehlendem Vulkan-Device ist whisper.cpp-internal — kein App-Code-Pfad. |
| Lokales LLM | **Embedded llama-cpp-2 0.1.146** mit Vulkan + `dynamic-link` ist seit Mai 2026 produktiver Standard (kein externer Daemon nötig, GGUF im VoiceTypeX-Prozess). Per-Mode-Switch via `local_engine = "embedded"` (Default) vs `"ollama"`. Ollama bleibt Opt-in für User, die einen eigenen Daemon betreiben. |
| Cloud-STT | xAI (One-Shot REST), OpenAI Whisper, Groq Whisper, Deepgram |
| Cloud-LLM | xAI Grok (Default `grok-4-fast-non-reasoning`), OpenAI GPT, Anthropic Claude |
| HTTP-Client | reqwest (rustls-tls) |
| Config | TOML (`serde` + `toml`) |
| Logging | tracing + tracing-subscriber + Ringbuffer für UI |
| Secrets | File (`~/.config/.../secrets.json`, chmod 0600) als Source of Truth, OS-Keychain best-effort Mirror |
| Audio-Cues | rodio |
| Datei-Watch | notify (Mode-Hot-Reload) |
| Repo & CI | GitLab (`.gitlab-ci.yml`) |

**Tauri-Plugins (verwendet):** global-shortcut (X11/Windows), store,
dialog, notification, os, fs, clipboard-manager, autostart.

**Tauri-Plugins (bewusst nicht verwendet):** tauri-plugin-updater
(kein Auto-Update geplant), tauri-plugin-shell.

Wire-Protokoll-Details der Cloud-Provider (Endpoints, Auth-Header,
Multipart-Reihenfolge, Response-Parsing) sind in
[`PROVIDERS.md`](PROVIDERS.md) dokumentiert.

## State-Machine

Eine globale State-Machine steuert die Pipeline. Implementiert in
[`src-tauri/src/core/state.rs`](../src-tauri/src/core/state.rs):

```
Idle ─► Recording ─► Transcribing ─┬─► Postprocessing ─► Injecting ─► Idle
                                   └─► Injecting ─► Idle  (Modus ohne LLM)
                                              ▲
                              Error ◄─ jede Stufe ─► Idle (Recovery)
```

`AppState::can_transition_to(...)` validiert jeden Übergang. Ungültige
Übergänge geben `VoiceTypeError::InvalidStateTransition` zurück und
werden in einem dedizierten Test geprüft.

Der `StateBus` nutzt `tokio::sync::watch` (letzter Wert reicht; mehrere
Subscriber, keine Backpressure-Probleme). Subscriber: Tray-Icon-Update,
Tray-Recording-Pulse, Frontend-Event-Emitter (`app://state`),
Overlay-State-Listener.

## Pipeline (Menü-Hotkey + Toggle)

[`src-tauri/src/pipeline/mod.rs`](../src-tauri/src/pipeline/mod.rs)
orchestriert die Stages. Es gibt **einen** globalen Hotkey
(`Settings.menu_hotkey`), der je nach Pipeline-State ein anderes
Verhalten triggert:

```
Hotkey-Press
   │
   ▼
handle_menu_hotkey(app, ctx)
   │
   ├─ State == Idle ─────────────► menu.show() + menu.set_focus()
   │                                Frontend zeigt Modus-Liste
   │                                User: ↑/↓, Enter, Esc
   │                                Enter → invoke("start_recording", {modeId})
   │
   ├─ State == Recording ───────► finish_recording_and_inject(active_mode)
   │
   └─ sonst ─────────────────────► ignoriert (Pipeline busy)

invoke("start_recording", { mode_id })
   │
   ▼
execute_mode(app, ctx, mode)   ─── State == Idle ? start : (Toggle-Stop)
   │
   ▼
start_recording(app, ctx, mode)
   │  • ctx.active_mode = Some(mode)   ── damit der Stop-Hotkey weiß, welcher Modus läuft
   │  • State → Recording
   │  • overlay.show()                 (Wayland-Fokus-Klau wird in finish() neutralisiert)
   │  • Start-Cue (kurzer Beep)
   │  • RecorderHandle::start()        (cpal-Stream im eigenen Thread)
   │
Hotkey-Press im Recording-State
   │
   ▼
finish_recording_and_inject(app, ctx, active_mode)
   │  • ctx.active_mode = None
   │  • State → Transcribing, Stop-Cue
   │  • recorder.stop_and_finalize() → 16 kHz Mono PCM s16 LE WAV
   │  • Transcriber::transcribe_oneshot
   │  • Falls processing != none: State → Postprocessing,
   │    Processor::process (Ollama oder Cloud-LLM)
   │  • State → Injecting
   │  • overlay.hide() + 80 ms Pause   (Fokus springt zurück zur Ziel-App)
   │  • Injector::inject               (libei auf Wayland, enigo+Clipboard sonst)
   │  • State → Idle
```

`AppContext.active_mode: Arc<Mutex<Option<Mode>>>` ist das Bindeglied
zwischen Start (kennt den Modus aus dem IPC-Aufruf) und Stop (Menü-
Hotkey weiß nicht selbst, welcher Modus läuft). Wird in
`start_recording` gesetzt, in `finish_recording_and_inject` direkt am
Anfang geleert.

UI-Trigger-Buttons in der Modi-Liste rufen `execute_mode` direkt — die
Toggle-Logik ist dieselbe wie beim Hotkey.

## Bearbeiten-Modi (Selektion → LLM → ersetzen/einfügen)

Modi mit `Mode.input == Selection` transformieren markierten Text statt
neues Diktat zu erzeugen. Die Pipeline ändert sich dafür minimal — kein
neuer State:

1. **Eager-Capture:** `handle_menu_hotkey` liest im `Idle`-Zweig die
   Selektion *vor* `menu.show()` (das Menü klaut den Fokus, das Lesen
   braucht aber die fokussierte Ziel-App). Gated auf Edit-Modus-Präsenz
   (`capture_selection_if_edit_modes`) — reine Diktier-Setups zahlen
   nichts. Das Ergebnis liegt in `AppContext.selection_buffer`.
2. **Lesen** geschieht über `TextInjector::read_selection()`. Auf Linux
   (X11 **und** Wayland) wird die **PRIMARY-Selection** direkt gelesen
   (`injection::read_primary_selection_linux` via arboard; X11 nativ,
   Wayland über wlr/ext-data-control) — fokus-unabhängig, ohne Ctrl+C,
   ohne die CLIPBOARD-Zwischenablage anzufassen. Auf Windows (keine
   PRIMARY-Selection) simuliert `ClipboardFallbackInjector` Ctrl+C und
   liest die Zwischenablage (sichern → Copy → lesen → wiederherstellen).
3. **Komposition** (`core::edit::compose_edit_input`): in
   `finish_recording_and_inject` wird bei `input == Selection` die
   Selektion + das transkribierte Diktat zu *einem* User-String
   (`<selected_text>…</selected_text>\n<instruction>…</instruction>`)
   gerahmt und als „transcript" an den `Processor` gegeben — der
   `Processor`-Trait bleibt unverändert.
4. **Output-Resolution** (`core::edit::resolve_output_action`): nach dem
   LLM wird `Mode.output` angewandt. Bei `Auto` parst der Sentinel
   (`@@REPLACE`/`@@APPEND`/`@@PREPEND` in Zeile 1) die Aktion, sonst
   greift `Mode.output_fallback`. Die Aktion fließt als
   `InjectOptions.action` in die Injection; `Append`/`Prepend` lassen den
   Injector vor dem Paste die Selektion kollabieren (Pfeiltaste).

Plattform-Reichweite und bekannte Grenzen: [`PLATFORMS.md`](PLATFORMS.md).

## Trait-Schichten

Provider- und Inject-kritische Funktionalität ist hinter Traits
abstrahiert. Plattform-Selektion zur Laufzeit (Linux nutzt
`WAYLAND_DISPLAY` zur Differenzierung in `core::session::is_wayland()`).

| Trait | Datei | Implementierungen |
|---|---|---|
| `Transcriber` | `transcription/mod.rs` | `LocalTranscriber` (whisper-rs), `XaiTranscriber`, `OpenAITranscriber`, `GroqTranscriber`, `DeepgramTranscriber` |
| `Processor` | `processing/mod.rs` | `LlamaEmbeddedProcessor` (embedded llama-cpp-2, **Default-Engine**), `OllamaProcessor` (lokaler Ollama-Daemon, Opt-in), `XaiProcessor`/`OpenAIProcessor` (via gemeinsamer `OpenAICompatibleClient`), `AnthropicProcessor` |
| `TextInjector` | `injection/mod.rs` | `ClipboardFallbackInjector` (X11/Windows: enigo Ctrl+V), `WaylandLibeiInjector` (Wayland: libei via xdg-desktop-portal.RemoteDesktop) — der Trait trägt zusätzlich `read_selection()` (Eingangsseite der Bearbeiten-Modi, siehe unten) |

**Hotkey-Registrierung** ist plattform-direkt (kein Trait, siehe
[CLAUDE.md §4.2](../CLAUDE.md)) und registriert **genau einen**
globalen Shortcut (`Settings.menu_hotkey`):

- X11/Windows: `pipeline::register_menu_hotkey()` registriert ihn direkt
  über `app.global_shortcut().on_shortcut()` aus
  `tauri-plugin-global-shortcut`. Nur `ShortcutState::Pressed` wird
  verarbeitet — Release-Events sind irrelevant (kein PTT mehr).
- Wayland: `pipeline::spawn_wayland_hotkey_session()` betreibt eine
  langlebige Portal-Session via
  `hotkey::linux_wayland::run_global_shortcuts_session()` mit einem
  einzigen `WaylandShortcutSpec`-Eintrag (`id = "open_menu"`). Events
  fließen als `HotkeyEvent` über einen broadcast-Channel zurück;
  Released-Events werden im Dispatcher gefiltert.

## AppContext (Tauri-State-Singleton)

[`core/app_context.rs`](../src-tauri/src/core/app_context.rs):

```rust
pub struct AppContext {
    pub state_bus: StateBus,
    pub modes: Arc<ModesRegistry>,
    pub recorder_slot: Arc<Mutex<Option<RecorderHandle>>>,
    pub active_mode: Arc<Mutex<Option<Mode>>>,         // welcher Modus läuft gerade?
    pub effective_menu_hotkey: Arc<RwLock<Option<String>>>, // Wayland: vom Portal zurückgegebener Trigger
    pub transcriber: Arc<dyn Transcriber>,             // App-Default-Transcriber (Local oder Cloud)
    pub local_transcriber: Arc<LocalTranscriber>,      // konkret für Streaming-Worker (Phase 2)
    pub local_llm_processor: Arc<LlamaEmbeddedProcessor>, // Embedded-LLM-Default-Processor (Phase 3b)
    pub extra_transcribers: Arc<Mutex<HashMap<String, Arc<LocalTranscriber>>>>, // Per-Mode-Whisper-Slot-Cache
    pub extra_llm_processors: Arc<Mutex<HashMap<String, Arc<LlamaEmbeddedProcessor>>>>, // Per-Mode-LLM-Slot-Cache
    pub active_streaming_handle: Arc<Mutex<Option<JoinHandle<()>>>>, // Phase-2-Streaming-Worker-Handle
    pub injector: Arc<dyn TextInjector>,
    pub selection_buffer: Arc<Mutex<Option<String>>>,  // eager-captured Selektion für Bearbeiten-Modi
    pub settings: Arc<RwLock<Settings>>,
    pub settings_path: PathBuf,
    pub log_buffer: LogRingBuffer,
    pub model_dir: PathBuf,
    pub modes_dir: PathBuf,
}
```

Per `app.manage(Arc<AppContext>)` als Tauri-Singleton verfügbar; jedes
IPC-Command zieht es via `tauri::State<'_, Arc<AppContext>>` heraus.

## Hot-Reload der Modi

`ModesRegistry` (`core/modes.rs`) hält die Modi-Liste in
`Arc<RwLock<Vec<Mode>>>`. Ein `notify::RecommendedWatcher` läuft in
einem eigenen Thread und reagiert auf `*.toml`-Änderungen im
`app_config_dir/modes/`. Bei jeder Änderung wird die ganze Liste neu
eingelesen und Subscriber per `broadcast::Sender<ModesEvent>`
benachrichtigt.

Beim ersten Start kopiert
[`core/default_modes.rs`](../src-tauri/src/core/default_modes.rs) die 6
in das Binary eingebetteten Default-TOMLs nach `app_config_dir/modes/`.

## Audio-Pipeline

`cpal::Stream` ist `!Send`. Lösung: dedicated Thread hält den Stream,
Send-Handle (`RecorderHandle`) kommuniziert per Channel mit dem Worker.
Audio wird als f32 gesammelt, beim Stop:

1. Stereo→Mono Mix (Mittelwert pro Frame)
2. `rubato::SincFixedIn` Resampling auf 16 kHz
3. `hound` WAV-Encoding (PCM s16 LE)

Der WAV-Buffer geht direkt zu `Transcriber::transcribe_oneshot`.

## Lokale STT-Pipeline (Phase 1, Mai 2026)

[`transcription/local.rs`](../src-tauri/src/transcription/local.rs) und
[`transcription/model_downloader.rs`](../src-tauri/src/transcription/model_downloader.rs):

**Whisper-Sampling (Default ab Phase 1):**
- `SamplingStrategy::BeamSearch { beam_size: 5, patience: 1.0 }` —
  ~2–3 % WER-Verbesserung auf deutschem Mehr-Satz-Diktat gegenüber
  Greedy, ~3× langsamer pro Decode-Step.
- `suppress_blank=true`, `no_speech_thold=0.6`, `temperature=0.0`
  mit `temperature_inc=0.2` als Fallback bei `logprob_thold`-Reissern.

**Silero-VAD v6.2.0:**
- Pfad: `app_config_dir/models/ggml-silero-v6.2.0.bin` (~885 kB).
- Wird beim ersten Whisper-Modell-Download als Best-effort
  mit-gezogen — fehlt das File, läuft Whisper transparent ohne VAD
  und loggt eine WARN-Zeile pro Aufruf.
- Defaults bewusst konservativer als upstream:
  `min_silence_duration_ms=500` (vs. 100), `speech_pad_ms=200`
  (vs. 30) — keine Mid-Sentence-Cuts, kein Konsonanten-Klipp.

**Modell-Slots** (`ModelSlot::from_setting`):

| Slot-String | Datei | Größe | Quelle |
|---|---|---|---|
| `large-v3-turbo-q8_0` *(Default)* | `ggml-large-v3-turbo-q8_0.bin` | ~874 MB | ggerganov/whisper.cpp |
| `large-v3-turbo-german-q5_0` | `ggml-model-q5_0.bin` | ~574 MB | cstr/whisper-large-v3-turbo-german-ggml (primeline-Fine-tune, Apache 2.0) |
| `large-v3-turbo-q5_0` | `ggml-large-v3-turbo-q5_0.bin` | ~547 MB | ggerganov/whisper.cpp |
| `small-q5_1` | `ggml-small-q5_1.bin` | ~181 MB | ggerganov/whisper.cpp |
| `large-v3-turbo` | `ggml-large-v3-turbo.bin` | ~1624 MB | ggerganov/whisper.cpp (F16) |

Alle Slots haben gepinte SHA-256-Hashes; Mismatch löst einen
Re-Download aus, akzeptiert nie ein verfälschtes File. Hashes stammen
aus den HF-Git-LFS-Pointern (`curl .../raw/main/<file> | head -3`).

**Bootstrap-Reihenfolge** (`lib.rs`): Settings werden vor der
Pipeline-Konstruktion geladen, der `LocalTranscriber` baut seinen
Modell-Pfad aus `settings.whisper_model_path` (Vorrang) bzw.
`ModelSlot::from_setting(&settings.whisper_default_slot).filename()`.
Vorher war der Pfad hardkodiert; User-Settings wurden ignoriert.

### Streaming (Phase 2)

Bei `mode.transcription = "local"` spawnt `start_recording` einen
parallelen Streaming-Decode-Worker. Dieser läuft während des
`Recording`-States parallel zur Aufnahme und liefert dem Overlay einen
live-aktualisierten Partial-Text, damit der User schon während des
Sprechens sieht, was Whisper versteht.

```
[Hold-to-record]──────────────────────────────[Release]
[Recorder cpal-Buffer waechst kontinuierlich]
[Streaming-Worker]
   ├─ t+2.0s: Snapshot → Convert 16k Mono → transcribe_streaming_pass → "Heute"
   ├─ t+2.8s: Snapshot → ...                                          → "Heute scheint"
   ├─ jeder neue Decode → emit app://partial-transcript
   │  (LocalAgreement-2 berechnet weiterhin Prefix-Konvergenz als Telemetrie)
   └─ Overlay zeigt "Heute scheint" unter "Hoere zu ..."
                                                       [Release]
                                                       └─►abort()
                                                       └─►Final-Pass (BeamSearch=5, kein audio_ctx)
                                                          ueberschreibt alles
```

**Gating**: Worker spawnt nur, wenn `mode.transcription ==
TranscriptionTarget::Local`. Cloud-Modi laufen weiter One-Shot nach
Stop-Hotkey — deren REST-Endpoints (xAI/OpenAI/Groq/Deepgram) haben
keine vergleichbare Streaming-Schnittstelle.

**Decode-Profil**: Streaming-Pässe nutzen `DecodeProfile::Streaming`:
Greedy-Sampling statt BeamSearch (3× schneller) plus `set_audio_ctx`
mit dynamisch-berechneter Frame-Zahl (`dynamic_audio_ctx_frames`)
für kurzes Audio (<30 s). Bei Audio ≥30 s wird der Trick weggelassen,
damit der Mel-Encoder nichts abschneidet. Der Final-Pass nach Stop
nutzt `DecodeProfile::Final` (BeamSearch + voller audio_ctx) — er
liefert den definitiven Text und überschreibt jede Partial-Ausgabe.

**LocalAgreement-2** (`transcription/local_agreement.rs`,
Machacek et al. arXiv 2307.14743): berechnet aus zwei
aufeinanderfolgenden Decodes den stabilen Wort-Prefix. Tokenisierung
auf Whitespace, Interpunktion bleibt am Wort haften.

**Aktuell nur Telemetrie, kein Emit-Gate.** Der ursprüngliche Plan war,
nur den stabilen Prefix ins Overlay zu emittieren; in der Praxis dauert
ein Streaming-Pass auf CPU-only-Hardware 8-12 s, sodass häufig kein
zweiter Decode vor dem Stop-Hotkey durchläuft und LA-2 alle Emits
blockiert hätte. Pragmatisch wird daher jeder neue, nicht-leere
Decode direkt emittiert (Text kann "wabern", wenn ein späterer Pass
einen früheren revidiert); der Final-Pass nach Stop überschreibt
ohnehin autoritativ. Die LA-2-Konvergenz wird als
`tracing::debug!`-Telemetrie weiterhin geloggt, um die Hardware-
Performance über Sessions hinweg beobachten zu können.

**Abort und State-Übergang**: `finish_recording_and_inject` ruft
`JoinHandle::abort()` auf den Streaming-Handle, bevor der Final-Pass
läuft. Das unterbricht den Loop am nächsten `.await`; CPU-Arbeit
innerhalb `spawn_blocking` läuft noch zu Ende, blockiert die Pipeline
aber nicht. Direkt nach `abort()` wird ein leerer
`app://partial-transcript`-Event geschickt, der das Overlay-Partial
löscht. Die State-Machine selbst ändert sich gegenüber Phase 1 nicht
— Streaming ist eine parallele Hilfs-Schleife, kein neuer State.

**Konfiguration** (alle Konstanten in `pipeline/mod.rs`):

| Konstante | Wert | Wirkung |
|---|---|---|
| `STREAMING_INITIAL_WAIT_MS` | 2000 | Warte vor erstem Decode, damit der Buffer Substanz hat |
| `STREAMING_INTERVAL_MS` | 800 | Decode-Frequenz |
| `STREAMING_MIN_AUDIO_SAMPLES` | 16000 | Decodes erst ab 1.0 s Audio (16 kHz) — kürzere Audios landen in whisper.cpp's "single timestamp ending" Skip-Pfad und liefern leere Outputs |

## Lokales LLM — zwei Pfade ab Phase 3b

`Mode.local_engine` waehlt pro Modus zwischen den beiden Pfaden:

### Embedded (`local_engine = "embedded"`) — **Default-Engine ab Mai 2026**

[`processing/embedded.rs`](../src-tauri/src/processing/embedded.rs):

- llama-cpp-2 0.1.146 mit Features `vulkan + sampler + dynamic-link`.
  `dynamic-link` Pflicht — sonst kollidieren statisch eingelinkte
  ggml-Versionen von whisper-rs-sys und llama-cpp-sys-2.
- `LlamaBackend::init()` einmalig per `OnceLock` Singleton.
- Modell-Cache hinter `Arc<RwLock<Option<LlamaModel>>>` (analog
  `LocalTranscriber`).
- Per `process()`-Call: frischer `LlamaContext` + `LlamaBatch`,
  Chat-Template aus dem GGUF (`model.chat_template(None)`),
  Sampling-Chain `penalties → top_p → temp → dist` (oder `greedy`
  bei temperature == 0).
- Modell-Pfad-Resolution: `Settings.llm_model_path` (Override) →
  `LlmModelSlot::from_setting(Settings.llm_default_slot).filename()`
  unter `app_config_dir/models/`.
- AppContext: `local_llm_processor: Arc<LlamaEmbeddedProcessor>`,
  beim Start konstruiert, Modell wird LAZY geladen.
- **Per-Mode-Slot-Override** via `Mode.embedded_llm_slot`: zeigt auf
  einen alternativen GGUF-Slot. Der Resolver in `pipeline/mod.rs`
  vergleicht gegen `Settings.llm_default_slot` — bei Gleichheit (oder
  `null`) wird der globale Processor wiederverwendet, sonst eine neue
  Instanz lazy in `AppContext.extra_llm_processors` (HashMap, Slot-
  Slug → Arc) gecached. Analog gibt es `Mode.whisper_model_slot` +
  `AppContext.extra_transcribers` für Whisper-Overrides.

### Ollama (`local_engine = "ollama"`) — Opt-in für externe Daemon-Nutzung

[`processing/local.rs`](../src-tauri/src/processing/local.rs):

- Default-Modell-Empfehlung fuer Ollama: `gemma3:4b` (DeepMind, Mar
  2025) — 140+ Sprachen, ~3 GB Footprint, Sweet-Spot für 8–16-GB-
  Geräte. Vorher `qwen2.5:7b`.
- **Sampling pro Modus** über `ProcessOpts.{temperature, top_p,
  repeat_penalty}`, gefüllt aus `Mode.temperature/top_p/
  repeat_penalty` (TOML-Felder). Default für "faithful rewrite, do
  not extend": 0.2 / 0.8 / 1.05.
- **`keep_alive` pro Request** aus `Settings.ollama_keep_alive`
  (Default `"5m"`, `"0"` für sofortiges Unload auf Memory-Pressure-
  Profilen, `"-1"` für unbegrenztes Warmhalten).
- Cloud-Processors (xAI/OpenAI/Anthropic) bekommen dieselben Sampling-
  Felder durchgereicht, soweit der Provider sie respektiert.

### Verzweigung im Pipeline-Code

`pipeline/mod.rs::run_local_processing` schaut auf `mode.local_engine`:
`"embedded"` (oder `None` — Default) → `resolve_embedded_llm(ctx, mode)`
(globaler Processor oder Cache-Lookup über `mode.embedded_llm_slot`),
`"ollama"` → `run_local_processing_ollama` (Ollama-HTTP-Call-Setup mit
keep_alive + `mode.ollama_model_tag`, Fallback auf deprecated
`local_llm_model`). Unbekannter Engine-Wert → `Mode`-Fehlermeldung.
Sampling-Felder werden in beide Pfade über `ProcessOpts.{temperature,
top_p, repeat_penalty, max_tokens}` durchgereicht.

**Bestehende User-TOMLs aus Phase 1/2** (mit `local_llm_model` oder
`ollama_model_tag` aber ohne `local_engine`) werden in
`Mode::migrate_deprecated_fields` beim Laden explizit auf
`local_engine = "ollama"` gesetzt — sonst würde der Default-Switch von
`"ollama"` auf `"embedded"` (Mai 2026) diese Modi auf den falschen
Engine-Pfad umleiten und mit „GGUF-Slot nicht gefunden" scheitern.

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
ensure_session()                   (lazy beim ersten Inject)
   │  • restore_token aus ~/.config/.../wayland_session.json laden (falls vorhanden)
   │  • ashpd::RemoteDesktop::create_session → select_devices(KEYBOARD, prior_token)
   │  • start(...) → bei erstem Mal: Permission-Dialog; bei gültigem Token: silent
   │  • connect_to_eis(...) → EIS-File-Descriptor
   │  • neuer restore_token wird auf Disk geschrieben
   │  • Worker-Thread spawnen mit dem FD + mpsc<KeyCommand>
   │       └─ EI-Handshake: HandshakeVersion → Connection → Seat → Device → Keyboard
   │       └─ bei Device::Resumed: start_emulating(seq, serial)
   │       └─ ready-Signal an tokio-Seite via oneshot
   ▼
60 ms sleep                        (Compositor-Roundtrip für set_selection)
   │
cmd_tx.send(KeyCommand::CtrlV)    (an libei-Worker)
   │
Worker: 4 Frames mit per-Frame-flush + 1 ms Pause:
   ▼
   keyboard.key(LEFTCTRL, Press) + frame + flush + sleep
   keyboard.key(V, Press)        + frame + flush + sleep
   keyboard.key(V, Released)     + frame + flush + sleep
   keyboard.key(LEFTCTRL, Released) + frame + flush
   ▼
80 ms sleep                        (libei-Verarbeitungszeit)
```

Pflicht-Disziplinen aus libei-Spec (siehe `protocol.xml` und Code-
Kommentare in `libei_worker.rs`):
- `start_emulating` exklusiv im `Resumed`-Handler (sonst silent
  discard)
- pro `frame` nur eine `key`-State-Change pro Taste
- `frame.time` strikt monoton, CLOCK_MONOTONIC-basiert (Rust
  `Instant::now()`)
- `sequence`-Counter monoton über App-Lebensdauer
- `Paused`-Event setzt `emulation_active` zurück; nächster `Resumed`
  startet neue Emulations-Sequenz mit incrementiertem `sequence`

## Windows

| Window | Zweck | Größe | Fokus | Pointer-Events | Position |
|---|---|---|---|---|---|
| `main` | Hauptfenster (Settings, Modes, Logs) | 960 × 720, resizable | ja | ja | OS-Default |
| `overlay` | Status-Indikator während Recording / Transcribing / … | 520 × 96, **non-resizable** | **nein** (`focus: false`) | **none** (CSS) | oben links 24,24 |
| `menu` | Modus-Auswahl per Pfeil-Navigation + Enter | 480 × 360, non-resizable, scrollbar bei vielen Modi | ja | ja | oben links 24,24 |

Alle drei Windows laden dieselbe `index.html`; das Routing erfolgt in
`src/main.tsx` per `?window=overlay` / `?window=menu` URL-Query, sonst
fällt es auf `App.tsx` (Hauptfenster).

### Sichtbarkeit ist Backend-gesteuert

Kein Frontend-show/hide — das Backend orchestriert beide
Sekundär-Windows in `pipeline/mod.rs` und `ipc/recording.rs`:

| Trigger | overlay | menu |
|---|---|---|
| `handle_menu_hotkey` (Idle) | — | `show()` + `set_focus()` |
| `start_recording` | `show()` | `hide()` (idempotent) |
| `finish_recording_and_inject` | `hide()` + 80 ms vor libei-Inject | — |
| `spawn_overlay_state_listener` bei Idle / Error | `hide()` | — |
| IPC `cancel_menu` (Esc) | — | `hide()` |

### Overlay-View (`src/views/Overlay.tsx`)

Schlank: abonniert `app://state`, rendert phasengerechten Status-Text
(*„Höre zu …"*, *„Transkribiere …"*, *„Verarbeite …"*, *„Füge ein …"*,
*„Fehler"*). Keine Tastatur-Interaktion, keine Pointer-Events
(CSS-Schutz, falls das Window mal sichtbar bleiben würde).

### Menu-View (`src/views/Menu.tsx`)

Liest Modi aus `useModesStore`, Cursor initial auf
`Settings.last_selected_mode_id` (1× Tippen für die häufigste Aktion).
Tastatur-Handler auf dem Root-Div:
- `↑` / `↓` / `Home` / `End` → Cursor
- `Enter` → `invoke("start_recording", { modeId })` (Backend versteckt
  das Menue-Window und zeigt das Overlay)
- `Esc` → `invoke("cancel_menu")` (Backend versteckt das Menue-Window)

### Wayland-Fokus-Quirk

`menu.set_focus()` wird auf Wayland-Compositors nicht garantiert
respektiert — der Compositor entscheidet. Auf KDE Plasma 6
funktioniert es zuverlässig, weil das Menü-Window mit `focus: true`
in der Tauri-Config geboren wird (stärkster Compositor-Hint). Auf
wlroots-Compositors wie Hyprland / Sway kann der Fokus dennoch
ausbleiben; dort ist die App ohnehin im Clipboard-Fallback-Modus
([`docs/PLATFORMS.md`](PLATFORMS.md) → *Hyprland / Sway / wlroots*).

### Wayland-Hotkey-Read-Back

Auf Wayland ist `Settings.menu_hotkey` nur ein **Vorschlag** an das
`xdg-desktop-portal.GlobalShortcuts`. KDE merkt sich nach dem ersten
`bind_shortcuts` die Zuweisung und ignoriert spätere
`preferred_trigger`-Werte; der User kann den Hotkey zudem in
*System-Settings → Globale Verknüpfungen → VoiceTypeX* nachjustieren.

`hotkey::linux_wayland::run_global_shortcuts_session` ruft daher nach
`bind_shortcuts` einmal `list_shortcuts` auf und schreibt das
`trigger_description` der ersten Action in
`AppContext.effective_menu_hotkey: Arc<RwLock<Option<String>>>`. Der
IPC-Command `get_effective_menu_hotkey` liest diesen Cache; das
Frontend (`Settings.tsx → MenuHotkeyField`) zeigt auf Wayland ein
read-only Feld mit dem effektiven Trigger + Hinweis auf die System-
Settings, auf X11 / Windows bleibt das Feld editierbar.

## Persistenz

| Was | Wo | Format |
|---|---|---|
| User-Settings (PTT, Modell-Slot, Audio-Gerät, …) | `~/.config/.../settings.json` | JSON, chmod 0644 |
| API-Keys (BYOK) | `~/.config/.../secrets.json` (Source of Truth) + OS-Keychain (Mirror) | JSON, chmod 0600 |
| Wayland `restore_token` | `~/.config/.../wayland_session.json` | JSON, chmod 0600 |
| Modi (Hot-Reload) | `~/.config/.../modes/*.toml` | TOML |
| Whisper-Modelle | `~/.config/.../models/*.bin` | GGML, SHA-256-verifiziert |
| Silero-VAD | `~/.config/.../models/ggml-silero-v6.2.0.bin` | GGML, SHA-256-verifiziert |
| GGUF-LLM-Modelle (Phase 3b) | `~/.config/.../models/*.gguf` | GGUF, SHA-256-verifiziert |

Settings + Token werden beim App-Start gelesen, nach jedem
Mutations-IPC geschrieben (siehe `Settings::load_or_default` /
`Settings::save` und `WaylandLibeiInjector::ensure_session`).

## Logging

Tracing-Stack mit drei Layern:
- `EnvFilter` (RUST_LOG kompatibel; Default `voicetypex=info,tauri=info,warn`)
- `fmt::layer()` für stdout (Dev)
- `LogRingBuffer::layer()` (in-memory, 500 Lines, polled von Logs-View)

CLAUDE.md §8 ist hart: Audio-/Transkript-/LLM-Antwort-Daten gehen
**niemals** ins Default-Logging. Diagnose-Logging-Toggle in den
Einstellungen würde additiv weitere Aufrufe aktivieren, keine
bestehenden filtern.

## Frontend

React 18 + TypeScript strict + Tailwind v3 + Zustand.

- **Views (`src/views/`):** Settings, Modes, Logs, Overlay, Menu
  (Menu und Overlay sind eigene Tauri-Windows aus derselben
  `index.html`, unterschieden per `?window=`-Query in `main.tsx`).
- **Components (`src/components/`):** Sidebar, ThemeToggle,
  Field, OnboardingWizard, ModeEditor, TestTranscriptionSection,
  AutoPasteTestSection, ApiKeysSection
- **Stores (`src/store/index.ts`):** UI (Tab-State + Theme-Choice),
  Settings, Modes — mit async Actions, einer pro IPC-Command
- **IPC-Wrapper (`src/lib/tauri.ts`):** einzige Stelle, die `invoke()`
  direkt benutzt; alle Commands namentlich exportiert

### Design-Tokens & Theme

- **Tokens leben als CSS-Custom-Properties** (RGB-Triplets) in
  `src/styles/globals.css` unter `:root` (Light) und `html.dark`
  (Dark). Tailwind mapt sie in `tailwind.config.ts` auf semantische
  Klassen: `bg-canvas/surface/elevated`, `text-fg/muted/faint`,
  `border-outline/strong`, `brand/brand-hover`, `status-*`.
- **Theme-Choice** (system/light/dark) lebt in `src/lib/theme.ts`,
  wird in localStorage persistiert und synchron vor React-Render in
  `main.tsx` angewendet (FOUC-Prevention). matchMedia-Listener
  reagiert auf OS-Theme-Wechsel, wenn die User-Wahl "system" ist.
- **Floating Windows** (Menu, Overlay) folgen System-Theme statt
  App-Setting: separate Tauri-Renderer haben eigenes localStorage,
  fallen auf "system" zurück. Bewusste Design-Entscheidung —
  ambient Notifications integrieren sich in den Desktop.

### Logo & Icons

- **Source-of-Truth**: SVG-Dateien in `src-tauri/icons/source/`. Das
  Markenzeichen ist Wave-to-Caret (4 Audio-Bars → I-Beam-Cursor) in
  Brand-Indigo (`#3D5AFE`). Tray-States bestehen aus 7 SVGs (logo +
  idle/recording/recording_pulse/processing/done/error).
- **Render-Pipeline** (manuell beim Icon-Update auszuführen):
  - `rsvg-convert -w N -h N source/logo.svg -o icon.png` für die
    Bundle-PNGs (32/128/256/512).
  - `magick PNG-Frames icon.ico` für `.ico` mit 16/32/48/256.
  - `rsvg-convert -w 64 -h 64 source/tray-X.svg -o tray/X.png` für
    jede der sechs Tray-States.
- **Web-Pendant**: `src/components/Logo.tsx` ist eine React-Komponente
  mit identischer Geometrie und `fill="currentColor"` — theme-aware
  via Tailwind, eingebunden im Sidebar-Header und im
  OnboardingWizard-Step-1-Hero. Bei Geometry-Anpassung
  beide Stellen (SVG-Source + Logo.tsx) synchron halten.

## Internationalisierung (i18n)

Eigener leichter Hook-Layer statt i18next — ~70 LOC, keine Runtime-Dep
ausser dem nativen `Intl.*`-API. Zielsprachen Release-1: `de`, `en`,
`fr`, `es`, `it`. Englisch ist Source-of-Truth (`src/i18n/locales/en.json`).

**Datenfluss:**

```
tauri_plugin_os::locale()        Settings.locale            useI18nStore
 (Backend, First-Run)     ──►   (persistiert, JSON)  ──►   (Frontend, Zustand)
                                       │                          │
                                       ▼                          ▼
                            pickSupported(raw)              useT() / useLocale()
                            ↓                                     │
                            "de"|"en"|"fr"|"es"|"it"              ▼
                                                            React-Components
```

- **Backend** macht die First-Run-Detection im `setup`-Hook
  ([`lib.rs::run`](../src-tauri/src/lib.rs)) und persistiert den rohen
  OS-Locale-String in `Settings.locale`. Single-Writer-Pattern, damit
  die drei Webviews (main, overlay, menu) nicht race-en. Deserialize-
  Validator filtert Hand-Edits in der Settings-Datei (BCP-47-aehnliche
  ASCII-Form, max. 35 Zeichen).
- **Frontend-Bootstrap** in [`main.tsx`](../src/main.tsx) holt
  `Settings.locale` via IPC, mappt es via `pickSupported()` auf eine
  der unterstuetzten Sprachen (Region-Suffix ignoriert,
  Fallback `en`), und setzt den `useI18nStore` *vor* dem React-Render.
- **`useT()`-Hook** ([`src/i18n/index.ts`](../src/i18n/index.ts)) ist
  via `useCallback([locale])` stabil. Eager-Loaded Dictionaries (5
  Locales × ~5 KB), Fallback-Kette `current → en → key`.
- **Plural-Regeln** kommen von `Intl.PluralRules` — Convention
  `key.one`, `key.other` (CLDR-Forms). Numerische `params.count`-Werte
  triggern automatisch die Plural-Auswahl.
- **Build-Gate** [`scripts/i18n-check.mjs`](../scripts/i18n-check.mjs)
  laeuft als `prebuild` und im Standalone-Target `pnpm i18n:check`:
  prueft Locale-Parity gegen `en.json`, scannt `t("...")`-Aufrufe auf
  Existenz (inkl. Plural-Base-Forms), und validiert Template-Literal-
  Praefixe (`t(\`app.tabs.${id}\`)` muss zu mindestens einem Key in
  `en.json` passen).

**Cross-Window-Sync:** Jedes Webview-Fenster (main, overlay, menu)
hat seinen eigenen `useI18nStore`. Der Sprach-Switcher in Settings
ruft drei Dinge auf:
1. `ipcSetSettings({...locale})` — persistiert in `Settings.locale`.
2. `useI18nStore.setState({locale})` — sofortiges UI-Update im
   eigenen Fenster.
3. `emit("i18n://locale-changed", {locale})` — Tauri-Event, das die
   anderen Fenster in [`src/main.tsx`](../src/main.tsx) ueber einen
   `listen()`-Subscriber empfangen und ihren Store aktualisieren.

**Was beim Locale-Wechsel NICHT live mitwandert** (bleibt bis App-
Neustart unveraendert):
- **Tray-Menue** (`Open settings`/`Quit` etc.) — Tauri-2 hat keinen
  Live-Swap fuer MenuItems; das Backend liest die Locale einmalig im
  `setup`-Hook und baut das Menue damit.
- **Default-Modi** (Name, Description, `system_prompt`) — sie werden
  beim First-Run einmalig nach `app_config_dir/modes/` kopiert und
  sind dann User-Content. Ein Locale-Wechsel danach faellt zurueck
  auf "User editiert oder loescht und re-bootstrappt manuell".
- **Backend-Error-Messages** in Bannern — die Strings kommen
  englisch aus dem Backend (Phase 4 hat sie auf normales Englisch
  normalisiert, statt vorher ASCII-transliteriertem Deutsch). Eine
  vollstaendige Error-Code-Internationalisierung (strukturierte
  `UserError`-Enum + Frontend-Mapping) ist als spaeterer Refactor
  vorgemerkt — User sehen die englischen Strings bisher unabhaengig
  von der UI-Locale.

**Pflicht beim Hinzufuegen neuer UI-Strings:**
1. Key in `en.json` ergaenzen (Source-of-Truth).
2. Uebersetzungen in `de.json`, `fr.json`, `es.json`, `it.json`
   parallel ergaenzen (sonst meldet `pnpm i18n:check` fehlende Keys).
3. Im React-Code via `t("namespace.key")` oder
   `t(\`namespace.${dynamic}\`)` einbinden (Template-Praefixe werden
   vom Build-Gate gegen den Key-Stand validiert).

## Hardware-Detection

[`core/hardware.rs`](../src-tauri/src/core/hardware.rs) detektiert beim
Start verfügbare Whisper-Backends (CPU, OpenBLAS, Vulkan, CUDA, Metal,
CoreML) durch Library-Probing. Empfehlung wird im Settings-UI gezeigt;
User kann das Cargo-Feature beim Build wählen — der Code zur Laufzeit
nutzt das, was der jeweilige Build liefert.

## Offene Punkte

Aktuell keine. macOS ist nicht im Scope; die `#[cfg(target_os = "macos")]`-
Stubs bleiben erhalten, damit der Code auf macOS kompiliert, aber sind
nicht aktives Implementierungs-Ziel.
