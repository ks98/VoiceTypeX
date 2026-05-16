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
| Lokales STT | whisper-rs mit Cargo-Feature-Backends (`fast-cpu`/OpenBLAS = Default; `gpu-vulkan`/`gpu-cuda`/`gpu-metal`/`gpu-coreml` opt-in) |
| Lokales LLM | Ollama via HTTP |
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
   ├─ State == Idle ─────────────► overlay.show() + overlay.set_focus()
   │                                emit("app://menu/open")
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

## Trait-Schichten

Provider- und Inject-kritische Funktionalität ist hinter Traits
abstrahiert. Plattform-Selektion zur Laufzeit (Linux nutzt
`WAYLAND_DISPLAY` zur Differenzierung in `core::session::is_wayland()`).

| Trait | Datei | Implementierungen |
|---|---|---|
| `Transcriber` | `transcription/mod.rs` | `LocalTranscriber` (whisper-rs), `XaiTranscriber`, `OpenAITranscriber`, `GroqTranscriber`, `DeepgramTranscriber` |
| `Processor` | `processing/mod.rs` | `OllamaProcessor` (local), `XaiProcessor`/`OpenAIProcessor` (via gemeinsamer `OpenAICompatibleClient`), `AnthropicProcessor` |
| `TextInjector` | `injection/mod.rs` | `ClipboardFallbackInjector` (X11/Windows: enigo Ctrl+V), `WaylandLibeiInjector` (Wayland: libei via xdg-desktop-portal.RemoteDesktop) |

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
    pub active_mode: Arc<Mutex<Option<Mode>>>,  // welcher Modus läuft gerade?
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
