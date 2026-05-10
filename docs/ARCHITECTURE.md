# Architektur

> Diese Datei dokumentiert die wichtigsten Strukturen und Datenflüsse.
> Detaillierte Implementierungs-Entscheidungen stehen als Kommentare im Code,
> Tech-Stack-Festlegungen und Konventionen in [`CLAUDE.md`](../CLAUDE.md).

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

## Pipeline (Push-to-Talk)

[`src-tauri/src/pipeline/mod.rs`](../src-tauri/src/pipeline/mod.rs)
orchestriert die Stages:

```
Hotkey-Press
   │
   ▼
handle_hotkey_pressed(app, ctx, mode)   ─── PTT vs. Toggle nach Settings
   │
   ▼
start_recording(app, ctx, mode)
   │  • State → Recording
   │  • overlay.show()              (Wayland-Fokus-Klau wird in finish() neutralisiert)
   │  • Start-Cue (kurzer Beep)
   │  • RecorderHandle::start()      (cpal-Stream im eigenen Thread)
   │
Hotkey-Release
   │
   ▼
finish_recording_and_inject(app, ctx, mode)
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

Toggle-Mode (Setting `ptt_mode = false`) ruft die gleichen Funktionen,
nur ohne Press/Release-Trennung — Hotkey-Druck toggelt zwischen Idle
und Recording.

## Trait-Schichten

Plattform-/Provider-kritische Funktionalität ist hinter Traits
abstrahiert. Plattform-Selektion zur Laufzeit (Linux nutzt
`WAYLAND_DISPLAY` zur Differenzierung).

| Trait | Datei | Implementierungen |
|---|---|---|
| `Transcriber` | `transcription/mod.rs` | `LocalTranscriber` (whisper-rs), `XaiTranscriber`, `OpenAITranscriber`, `GroqTranscriber`, `DeepgramTranscriber` |
| `Processor` | `processing/mod.rs` | `OllamaProcessor` (local), `XaiProcessor`/`OpenAIProcessor` (via gemeinsamer `OpenAICompatibleClient`), `AnthropicProcessor` |
| `TextInjector` | `injection/mod.rs` | `ClipboardFallbackInjector` (X11/Windows: enigo Ctrl+V), `WaylandLibeiInjector` (Wayland: libei via xdg-desktop-portal.RemoteDesktop) |
| `HotkeyManager` | `hotkey/mod.rs` | tauri-plugin-global-shortcut (X11/Windows), xdg-desktop-portal.GlobalShortcuts via ashpd (Wayland) |

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

## Overlay-Window

Zweites Tauri-Window (`label: "overlay"`), oben links 24 px vom Rand
entfernt, 520 × 96 px, transparent + alwaysOnTop + decorations off +
focus: false + `pointer-events-none` als CSS-Schutz.

**Sichtbarkeit ist Backend-gesteuert** (kein Frontend-show/hide):
- `start_recording` ruft `overlay.show()`
- `finish_recording_and_inject` ruft `overlay.hide()` direkt vor dem
  libei-Inject (mit 80 ms Pause), damit der Tastatur-Fokus zurück zur
  Ziel-App springt
- `spawn_overlay_state_listener` versteckt zusätzlich bei `Idle`/`Error`
  als Cleanup

Frontend (`src/views/Overlay.tsx`) abonniert `app://state` und rendert
phasengerechten Status-Text. Window-Routing zwischen Hauptfenster und
Overlay über `?window=overlay` URL-Query in `src/main.tsx`.

## Persistenz

| Was | Wo | Format |
|---|---|---|
| User-Settings (PTT, Modell-Slot, Audio-Gerät, …) | `~/.config/.../settings.json` | JSON, chmod 0644 |
| API-Keys (BYOK) | `~/.config/.../secrets.json` (Source of Truth) + OS-Keychain (Mirror) | JSON, chmod 0600 |
| Wayland `restore_token` | `~/.config/.../wayland_session.json` | JSON, chmod 0600 |
| Modi (Hot-Reload) | `~/.config/.../modes/*.toml` | TOML |
| Whisper-Modelle | `~/.config/.../models/*.bin` | GGML |

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

- **Views (`src/views/`):** Settings, Modes, Logs, Overlay
- **Components (`src/components/`):** ModeEditor, OnboardingWizard,
  TestTranscriptionSection, AutoPasteTestSection, ApiKeysSection
- **Stores (`src/store/index.ts`):** UI (Tab-State), Settings,
  Modes — mit async Actions, einer pro IPC-Command
- **IPC-Wrapper (`src/lib/tauri.ts`):** einzige Stelle, die `invoke()`
  direkt benutzt; alle Commands namentlich exportiert

## Hardware-Detection

[`core/hardware.rs`](../src-tauri/src/core/hardware.rs) detektiert beim
Start verfügbare Whisper-Backends (CPU, OpenBLAS, Vulkan, CUDA, Metal,
CoreML) durch Library-Probing. Empfehlung wird im Settings-UI gezeigt;
User kann das Cargo-Feature beim Build wählen — der Code zur Laufzeit
nutzt das, was der jeweilige Build liefert.

## Offene Punkte für spätere Phasen

- **Phase 6 — macOS + Distribution-Hardening:**
  - macOS-Implementierungen (CGEvent-Inject, NSStatusItem-Tray,
    TCC-/Accessibility-Permissions im Onboarding)
  - Signierte Installer (Apple Notarization, Windows Authenticode)
  - Auto-Update via tauri-plugin-updater mit signierten Manifesten
