# Architektur

> Diese Datei dokumentiert die wichtigsten Strukturen und Datenflüsse.
> Detaillierte Implementierungs-Entscheidungen stehen als Kommentare im Code,
> Tech-Stack-Festlegungen in [`CLAUDE.md`](../CLAUDE.md).

## State-Machine

Eine einzige globale State-Machine steuert die Pipeline. Implementiert in
[`src-tauri/src/core/state.rs`](../src-tauri/src/core/state.rs):

```
Idle ─► Recording ─► Transcribing ─┬─► Postprocessing ─► Injecting ─► Idle
                                   └─► Injecting ─► Idle  (Modus ohne LLM)
                                              ▲
                              Error ◄─ jede Stufe ─► Idle (Recovery)
```

`AppState::can_transition_to(...)` validiert jeden Übergang. Ungültige
Übergänge geben `VoiceTypeError::InvalidStateTransition` zurück und werden
in einem dedizierten Test geprüft.

Der `StateBus` nutzt `tokio::sync::watch` (letzter Wert reicht; mehrere
Subscriber, keine Backpressure-Probleme). Das Tray-Icon und – später – die
Frontend-Anzeige abonnieren ihn.

## Pipeline

[`src-tauri/src/pipeline/mod.rs`](../src-tauri/src/pipeline/mod.rs)
orchestriert die Stages:

```
Hotkey-Press
   │
   ▼
execute_mode(app, ctx, mode)   ─── Toggle-Logik nach AppState
   │
   ├─ Idle      ── start_recording: Cue + RecorderHandle::start
   │
   └─ Recording ── finish_recording_and_inject:
                       Cue + stop_and_finalize  (16 kHz Mono WAV)
                       └► Transcriber::transcribe_oneshot
                              └► Processor::process  (skip wenn processing=none)
                                     └► Injector::inject  (Clipboard-Fallback)
                                            └► Idle
```

In Phase 1 ist nur der Modus `exakt` (lokal STT, kein LLM) end-to-end
verdrahtet. Die anderen Modi haben registrierte Hotkeys, antworten aber
mit Notification "noch nicht implementiert" – DoD §6.1.

## Trait-Schichten

Jede plattform-/provider-kritische Funktionalität ist hinter einem Trait
abstrahiert. Plattform-Selektion zur Laufzeit (Linux nutzt
`WAYLAND_DISPLAY` zum Differenzieren).

| Trait | Datei | Phase-1-Implementierung |
|---|---|---|
| `Transcriber` | `transcription/mod.rs` | `LocalTranscriber` (whisper-rs) |
| `Processor` | `processing/mod.rs` | nur Stubs (Phase 2) |
| `TextInjector` | `injection/mod.rs` | `ClipboardFallbackInjector` (alle Plattformen) |
| `HotkeyManager` | `hotkey/mod.rs` | per `tauri-plugin-global-shortcut` |

Cloud-Provider sind als Trait-konforme Stubs vorhanden (`unimplemented`/
`Err(...)`); Phase 2 ersetzt nur die Stub-Body, nicht die Trait-Form.

## AppContext (Tauri-State-Singleton)

[`core/app_context.rs`](../src-tauri/src/core/app_context.rs):

```rust
pub struct AppContext {
    pub state_bus: StateBus,
    pub modes: Arc<ModesRegistry>,
    pub recorder_slot: Arc<Mutex<Option<RecorderHandle>>>,
    pub transcriber: Arc<dyn Transcriber>,
    pub injector: Arc<dyn TextInjector>,
    pub settings: Arc<RwLock<Settings>>,
    pub log_buffer: LogRingBuffer,
    pub model_dir: PathBuf,
    pub modes_dir: PathBuf,
}
```

Per `app.manage(Arc<AppContext>)` als Tauri-Singleton verfügbar; jedes
IPC-Command zieht es via `tauri::State<'_, Arc<AppContext>>` heraus.

## Hot-Reload der Modi

`ModesRegistry` (`core/modes.rs`) hält die Modi-Liste in
`Arc<RwLock<Vec<Mode>>>`. Ein `notify::RecommendedWatcher` läuft in einem
eigenen Thread und reagiert auf `*.toml`-Änderungen im `app_config_dir/modes/`.
Bei jeder Änderung wird die ganze Liste neu eingelesen und Subscriber per
`broadcast::Sender<ModesEvent>` benachrichtigt.

Beim ersten Start kopiert
[`core/default_modes.rs`](../src-tauri/src/core/default_modes.rs) die 6 in das
Binary eingebetteten Default-TOMLs nach `app_config_dir/modes/`.

## Audio-Pipeline

`cpal::Stream` ist `!Send`. Lösung: dedicated Thread hält den Stream,
Send-Handle (`RecorderHandle`) kommuniziert per Channel mit dem Worker. Audio
wird als f32 gesammelt, beim Stop:

1. Stereo→Mono Mix (Mittelwert pro Frame)
2. `rubato::SincFixedIn` Resampling auf 16 kHz
3. `hound` WAV-Encoding (PCM s16 LE)

Der WAV-Buffer geht direkt zu `Transcriber::transcribe_oneshot`.

## Logging

Tracing-Stack mit drei Layern:
- `EnvFilter` (RUST_LOG kompatibel)
- `fmt::layer()` für stdout (Dev)
- `LogRingBuffer::layer()` (in-memory, 500 Lines, polled von Logs-View)

CLAUDE.md §8 ist hart: Audio-/Transkript-/LLM-Antwort-Daten gehen
**niemals** ins Default-Logging. Phase 1 hat keine solchen Log-Aufrufe;
ein "Diagnose-Logging"-Toggle würde additiv weitere Aufrufe aktivieren,
keine bestehenden filtern.

## Frontend

React 18 + TypeScript strict + Tailwind v3 + Zustand.

3 Views (`src/views/`): Settings, Modes (read-only), Logs.
3 Stores (`src/store/index.ts`): UI (Tab-State), Settings (mit
Audio-Geräte-Liste), Modes.
IPC-Wrapper in `src/lib/tauri.ts` ist die einzige Stelle, die `invoke()` direkt
verwendet — alle Commands sind dort namentlich.

## Offene Punkte für spätere Phasen

- **Phase 2**: xAI End-to-End (STT + LLM). `processing/cloud/openai_compatible.rs`
  als reqwest-Implementierung, `transcription/cloud/xai.rs` mit multipart/form-data.
- **Phase 4**: Streaming-STT (xAI WebSocket), Voice-Activity-Detection,
  Hot-Swap des Audio-Geräts, strukturierte Fehler-Taxonomie.
- **Phase 5**: Wayland (`xdg-desktop-portal.GlobalShortcuts` + `libei`).
- **Phase 6**: macOS (CGEvent-Injection, signierter Installer, Auto-Update).
