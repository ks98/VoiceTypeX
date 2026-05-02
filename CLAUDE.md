# VoiceTypeX — Bauanleitung für Claude Code

> Verwende diese Datei als initialen Prompt für Claude Code **und** behalte sie als `CLAUDE.md` im Repo-Root. Lies sie zu Beginn jeder neuen Aufgabe erneut.

---

## 1. Projektübersicht

**VoiceTypeX** ist ein plattformübergreifendes Desktop-Werkzeug, das Diktate in Text verwandelt und ihn an der aktuellen Cursor-Position einfügt — überall, in jeder App.

**Kernablauf:**
1. Der User drückt einen globalen Hotkey (pro Modus konfigurierbar).
2. Audio wird vom Standard-Mikrofon mit klarem visuellen/akustischen Hinweis aufgenommen.
3. Audio wird transkribiert (lokal via whisper.cpp **oder** über einen Cloud-STT-Provider).
4. Das Transkript wird durch ein LLM gemäß dem aktiven **Modus** nachbearbeitet (ein benanntes Prompt-Template) — lokales LLM via Ollama **oder** Cloud-LLM. Der Modus "Exaktes Diktat" überspringt diesen Schritt.
5. Der finale Text wird **an der aktuellen Cursor-Position eingefügt**, als hätte der User ihn getippt.

**Wichtigste sichtbare Funktionen:**
- System-Tray-Icon mit Statusfarben (idle / aufnehmend / verarbeitend / fertig / fehler) und Kontextmenü (Öffnen, Beenden, schneller Modus-Wechsel).
- Settings-UI für: API-Keys (BYOK), Standard-Modus, Hotkeys, Audio-Gerät, lokale Modellpfade, Modus-Editor.
- Wird mit 6 vorinstallierten Modi (deutsch) ausgeliefert. User können eigene über TOML-Dateien hinzufügen — keine Code-Änderung nötig.
- Drei Ausführungs-Kombinationen pro Modus: `local/local`, `local/cloud`, `cloud/cloud` (Transkription / Nachbearbeitung).

**Datenschutz-Positionierung:** Lokal = kostenlos, 100 % offline, geringere Qualität. Cloud = Premium, höchste Qualität. Der User sieht jederzeit, welcher Pfad aktiv ist.

---

## 2. Tech-Stack — FESTGELEGT

Schlage **keine** Alternativen vor. Entscheidungen getroffen:

| Schicht | Wahl |
|---|---|
| App-Framework | **Tauri 2** (neueste stabile Version) |
| Backend | **Rust** (Edition 2021+) |
| Frontend | **React 18 + TypeScript + Vite** |
| Styling | **TailwindCSS** + **shadcn/ui** Komponenten |
| Frontend-State | **Zustand** (klein, einfach) |
| Async-Runtime (Rust) | **tokio** |
| Audio-Aufnahme | **cpal** + **hound** (WAV-Buffer) |
| Lokales STT | **whisper-rs** (whisper.cpp Rust-Bindings) |
| Lokales LLM | **Ollama** via HTTP (kein direktes Linken von llama.cpp) |
| Cloud-STT (initial, primär) | **xAI Speech-to-Text** (`POST https://api.x.ai/v1/stt`) |
| Cloud-STT (initial, weitere) | Groq Whisper, OpenAI Whisper, Deepgram |
| Cloud-LLM (initial, primär) | **xAI Grok** (OpenAI-API-kompatibel, Base-URL `https://api.x.ai/v1`) |
| Cloud-LLM (initial, weitere) | Anthropic Claude, OpenAI GPT |
| Config-Format | **TOML** via `serde` + `toml` Crate |
| Logging | `tracing` + `tracing-subscriber` |
| HTTP-Client | `reqwest` (mit `rustls-tls`, keine OpenSSL-Abhängigkeit), für Streaming-STT zusätzlich `tokio-tungstenite` |
| Secret-Storage | OS-Keychain via `keyring` Crate (BYOK API-Keys nie im Klartext) |
| Audio-Wiedergabe (Cues) | `rodio` (MIT) — kurze WAV-Beeps bei Aufnahme-Start/-Stopp |
| Audio-Resampling | `rubato` (MIT) — von Mikrofon-Native-Rate (44.1 / 48 kHz Stereo) auf Whisper-Eingabe (16 kHz Mono) |
| Datei-Watch (Mode-Hot-Reload) | `notify` (MIT) |
| Repo-Hosting & CI | **GitLab** — CI-Konfiguration in `.gitlab-ci.yml`, kein `.github/workflows/` |

**Hinweis zu xAI als Provider:** xAI bietet sowohl LLM (Grok) als auch Speech-to-Text an, beides erreichbar mit demselben API-Key. Die Konsequenz für die Implementierung:

- **xAI LLM** ist OpenAI-Chat-Completions-kompatibel — derselbe HTTP-Client-Code kann für xAI und OpenAI verwendet werden, nur Base-URL und Model-IDs (`grok-4`, `grok-3`, …) unterscheiden sich. `processing/cloud/xai.rs` ist daher ein dünner Wrapper über einen gemeinsamen `OpenAICompatibleClient`.
- **xAI STT** ist **NICHT** OpenAI-Whisper-kompatibel. Eigenes `multipart/form-data`-Request-Format, eigene Response-Struktur (`text`, `language`, `duration`, `words[]` mit Word-Level-Timestamps). `transcription/cloud/xai.rs` muss daher eine eigenständige Implementierung sein — kein gemeinsamer STT-Client mit OpenAI/Groq.
- **Streaming-STT (Phase 4+):** xAI bietet zusätzlich `wss://api.x.ai/v1/stt` mit Live-Transkription während der Aufnahme (interim results, ~500 ms Frequenz). Aktuell außerhalb des Scope, aber die Architektur soll später nicht umgebaut werden müssen — siehe Abschnitt 4.7.

**Zu verwendende Tauri-Plugins:**
- `tauri-plugin-global-shortcut`
- `tauri-plugin-store` (Settings-Persistenz)
- `tauri-plugin-dialog`
- `tauri-plugin-notification`
- `tauri-plugin-os`
- `tauri-plugin-fs`
- `tauri-plugin-clipboard-manager`
- `tauri-plugin-autostart`
- `tauri-plugin-updater`
- Eingebaute Tray- + Menu-APIs (Tauri 2)

### 2.1 Default-STT-Modell (lokales Whisper)

Beim ersten Start lädt VoiceTypeX automatisch das Default-Modell aus dem Hugging-Face-Repo `ggerganov/whisper.cpp` herunter (mit SHA-Hash-Verifikation), legt es in `app_data_dir()` ab und kann von dort referenziert werden.

| Slot | GGML-Datei | Größe | Deutsche Qualität (WER) | RTF auf 8-Core CPU + AVX2 |
|---|---|---|---|---|
| **Default** | `ggml-large-v3-turbo-q5_0.bin` | ~547 MB | ~5–7 % | ~0.4–0.7 |
| **Spar-Fallback** | `ggml-small-q5_1.bin` | ~181 MB | ~12 % | ~0.2 |
| Optional | `ggml-large-v3-turbo.bin` (unquantisiert) | ~1.6 GB | ~5 % | ~0.6–0.9 |

Begründung der Wahl:
- `large-v3-turbo` (4 Decoder-Layer statt 32 in `large-v3`) liefert ca. medium-Niveau Qualität bei deutlich niedrigerer Latenz als `medium`.
- Q5_0-Quantisierung reduziert Disk und RAM um ~50 % bei < 1 % WER-Verschlechterung — in der whisper.cpp-Community als praktisch verlustfrei etabliert.
- `ggml-base.bin` und `ggml-tiny.bin` sind für deutsches Diktat **nicht akzeptabel** (WER ≥ 20 %) und dürfen nicht als Default gewählt werden, auch wenn sie schneller sind.

Settings-UI verpflichtend: Modell-Dropdown mit den drei genannten Slots plus "eigener Pfad" + "Test-Transkription"-Button, der ein 5-Sekunden-Sample misst und die tatsächliche RTF des Systems anzeigt. Damit erhält der User belastbare Performance-Zahlen für seinen konkreten Rechner, statt sich auf die Tabelle oben verlassen zu müssen.

---

## 3. Repository-Struktur

Lege zu Beginn von Phase 1 exakt diese Struktur an:

```
voicetypex/
├── CLAUDE.md                       # diese Datei
├── README.md
├── LICENSE                         # GPL-3.0 (Volltext von gnu.org)
├── COPYING                         # Alias zu LICENSE, GPL-Konvention
├── .gitignore
├── .editorconfig
├── rust-toolchain.toml             # auf stable pinnen
├── package.json                    # Frontend-Dependencies
├── pnpm-lock.yaml                  # pnpm verwenden
├── vite.config.ts
├── tailwind.config.ts
├── tsconfig.json
├── index.html
├── src/                            # React-Frontend
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/
│   ├── views/
│   │   ├── Settings.tsx
│   │   ├── ModeEditor.tsx
│   │   └── Logs.tsx
│   ├── store/                      # Zustand-Stores
│   ├── lib/                        # Frontend-Utils, IPC-Wrapper
│   └── styles/
├── src-tauri/                      # Rust-Backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── capabilities/
│   │   └── default.json
│   ├── icons/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── core/
│       │   ├── mod.rs
│       │   ├── state.rs            # die App-State-Machine
│       │   ├── config.rs
│       │   ├── modes.rs            # Mode-Struct, Laden aus TOML
│       │   └── error.rs
│       ├── audio/
│       │   ├── mod.rs
│       │   ├── recorder.rs         # cpal-basierter Recorder
│       │   └── vad.rs              # Platzhalter für später
│       ├── transcription/
│       │   ├── mod.rs              # `Transcriber` Trait
│       │   ├── local.rs            # whisper-rs Implementierung
│       │   └── cloud/
│       │       ├── mod.rs
│       │       ├── xai.rs          # xAI STT (eigene API, multipart/form-data)
│       │       ├── openai.rs       # OpenAI Whisper API
│       │       ├── groq.rs         # Groq Whisper (OpenAI-Whisper-kompatibel)
│       │       └── deepgram.rs
│       ├── processing/
│       │   ├── mod.rs              # `Processor` Trait
│       │   ├── local.rs            # Ollama HTTP-Client
│       │   └── cloud/
│       │       ├── mod.rs
│       │       ├── openai_compatible.rs  # gemeinsamer Client für OpenAI-Chat-Completions-kompatible Provider
│       │       ├── xai.rs          # xAI Grok (nutzt openai_compatible.rs)
│       │       ├── openai.rs       # OpenAI GPT (nutzt openai_compatible.rs)
│       │       └── anthropic.rs    # Anthropic Claude (eigene API)
│       ├── injection/
│       │   ├── mod.rs              # `TextInjector` Trait + Factory
│       │   ├── windows.rs          # SendInput
│       │   ├── macos.rs            # CGEvent (Stub in Phase 1)
│       │   ├── linux_x11.rs        # XTest
│       │   ├── linux_wayland.rs    # libei (Stub in Phase 1)
│       │   └── clipboard_fallback.rs  # universeller Fallback
│       ├── hotkey/
│       │   ├── mod.rs              # `HotkeyManager` Trait + Factory
│       │   ├── windows.rs
│       │   ├── macos.rs            # Stub
│       │   ├── linux_x11.rs
│       │   └── linux_wayland.rs    # xdg-desktop-portal (Stub Phase 1)
│       ├── tray/
│       │   ├── mod.rs
│       │   └── icon.rs
│       ├── ipc/                    # Tauri-Command-Handler
│       │   ├── mod.rs
│       │   ├── settings.rs
│       │   ├── modes.rs
│       │   ├── recording.rs
│       │   └── diagnostics.rs
│       └── secrets.rs              # Keychain-Wrapper für API-Keys
├── modes/                          # mit der App ausgelieferte Standard-Modi
│   ├── exaktes_diktat.toml
│   ├── korrigierendes_diktat.toml
│   ├── foermliche_email.toml
│   ├── slack_teams.toml
│   ├── github_issue.toml
│   └── claude_code_anweisung.toml
├── assets/                         # eingebettete Binär-Assets (in die App gebundelt)
│   ├── cue_start.wav               # kurzer Beep bei Aufnahme-Start (~80 ms)
│   └── cue_stop.wav                # kurzer Beep bei Aufnahme-Stopp (~80 ms)
├── .gitlab-ci.yml                  # CI-Pipeline (Repo liegt auf GitLab, NICHT GitHub)
└── docs/
    ├── ARCHITECTURE.md
    ├── PLATFORMS.md                # plattformspezifische Notizen
    └── MODES.md                    # wie man einen Modus schreibt
```

---

## 4. Architektur-Prinzipien

### 4.1 Die State-Machine
Ein einzelnes `AppState`-Enum steuert alles:

```
Idle → Recording → Transcribing → Postprocessing → Injecting → Idle
                                                       ↓
                                                    Error → Idle
```

Implementiere als Typestate oder als `enum` mit einem `tokio::sync::watch` Channel, den Tray, Frontend und jedes zukünftige Overlay abonnieren können. Jeder State-Übergang aktualisiert die Tray-Icon-Farbe.

### 4.2 Trait-basierte Plattform-Abstraktion
Die zwei plattformkritischen Bereiche bekommen eigene Traits, mit einer Implementierung pro OS, zur Laufzeit ausgewählt:

```rust
#[async_trait]
pub trait HotkeyManager: Send + Sync {
    async fn register(&self, id: &str, accelerator: &str) -> Result<()>;
    async fn unregister(&self, id: &str) -> Result<()>;
    fn events(&self) -> tokio::sync::broadcast::Receiver<HotkeyEvent>;
}

#[async_trait]
pub trait TextInjector: Send + Sync {
    async fn inject(&self, text: &str, opts: InjectOptions) -> Result<()>;
    fn capabilities(&self) -> InjectorCapabilities;
}
```

Eine Factory in `injection/mod.rs` und `hotkey/mod.rs` wählt die richtige Implementierung über:
1. `cfg!(target_os = ...)`
2. Auf Linux: erkenne `WAYLAND_DISPLAY` vs. `DISPLAY` zur Laufzeit.

### 4.3 Ein Modus ist nur eine TOML-Datei
Ein `Mode` ist Name + Hotkey + Transkriptionsziel + Verarbeitungsziel + System-Prompt. Der Modus-Loader beobachtet `modes/` auf Änderungen und lädt heiß nach. Strikt mit serde validieren und Fehler im UI sichtbar machen.

### 4.4 BYOK + Keychain
API-Keys liegen im OS-Keychain via `keyring` Crate. Sie werden **niemals** im Klartext auf Disk gespeichert, niemals geloggt, niemals in Fehlerberichten verschickt. Das Settings-UI zeigt maskierte Werte und "Verbindung testen"-Buttons. Pro unterstütztem Provider ein eigener Keychain-Eintrag — wichtig: xAI nutzt **denselben** API-Key für STT und LLM, daher gibt es nur einen `xai`-Eintrag, der von beiden Modulen gelesen wird.

### 4.5 Clipboard-Fallback als Standardstrategie
Für die erste Iteration **bevorzuge auf jeder Plattform den Clipboard-Paste-Pfad** für die Text-Injection:
1. Aktuellen Clipboard-Inhalt sichern.
2. Clipboard auf den neuen Text setzen.
3. Den Paste-Shortcut der Plattform senden.
4. Nach ~150 ms den vorherigen Clipboard-Inhalt wiederherstellen.

Direkte Tastendruck-Injection (SendInput / XTest / libei) wird als Opt-in pro Modus hinzugefügt (`injection_method = "keystrokes"`), nützlich für "Exaktes Diktat", aber in manchen Ziel-Apps unzuverlässig.

### 4.6 Cloud-Provider-Abstraktion — getrennt für STT und LLM

**Cloud-LLM:** Alle Cloud-LLM-Provider implementieren denselben `Processor`-Trait. Da xAI und OpenAI dieselbe Chat-Completions-API verwenden, gibt es eine gemeinsame interne Struktur `OpenAICompatibleClient` in `processing/cloud/openai_compatible.rs`, die per Konstruktor Base-URL, Default-Model und API-Key bekommt. `xai.rs` und `openai.rs` sind dünne Wrapper, die diese Struktur instanziieren. `anthropic.rs` ist eine separate Implementierung, da das Messages-API von Anthropic eigene Konventionen hat (System-Prompt-Feld, Content-Blöcke). Modelle werden pro Modus konfiguriert (`cloud_llm_model = "grok-4"`); fehlt der Eintrag, gilt der Provider-Default.

**Cloud-STT:** Hier gibt es **keinen** gemeinsamen Wrapper, weil die Provider unterschiedliche APIs verwenden:
- `xai.rs` — eigenes API (`POST https://api.x.ai/v1/stt`, `multipart/form-data` mit `file` als letztem Field, Response mit `text`/`language`/`duration`/`words[]`).
- `openai.rs` und `groq.rs` — beide OpenAI-Whisper-kompatibel (`POST .../v1/audio/transcriptions`), können einen gemeinsamen Helper teilen, falls Groq beibehalten wird.
- `deepgram.rs` — eigenes API (Deepgram-eigene Struktur).

Jeder STT-Provider implementiert den `Transcriber`-Trait direkt; kein Versuch, eine künstliche Gemeinsamkeit zu konstruieren, die es real nicht gibt.

### 4.7 Streaming-fähige Architektur (für spätere Phasen)
Der `Transcriber`-Trait wird so designt, dass er sowohl **One-Shot** (komplettes Audio rein, kompletter Text raus) als auch **Streaming** (Audio-Chunks rein, Text-Chunks raus) unterstützen kann:

```rust
pub enum TranscriptionMode {
    OneShot,
    Streaming,
}

pub enum TranscriptionEvent {
    Partial { text: String, is_final: bool },
    Done { text: String, duration_ms: u32 },
    Error(anyhow::Error),
}

#[async_trait]
pub trait Transcriber: Send + Sync {
    fn supports(&self) -> &[TranscriptionMode];
    async fn transcribe_oneshot(&self, audio: &[u8], opts: TranscribeOpts) -> Result<String>;
    /// Returns a Stream of TranscriptionEvent. Default impl: error if not supported.
    async fn transcribe_stream(&self, /* audio channel */) -> Result<BoxStream<'static, TranscriptionEvent>>;
}
```

In Phase 1 implementiert nur `local.rs` `transcribe_oneshot` und meldet Streaming als nicht unterstützt. In Phase 2 kommt der `xai.rs`-One-Shot-Pfad dazu. In Phase 4 wird der xAI-WebSocket-Streaming-Pfad eingebaut, ohne dass Trait-Signaturen sich ändern müssen.

---

## 5. Die Standard-Modi (deutsche UI-Strings, mit der App ausgeliefert)

Alle sechs TOML-Dateien liegen unter `modes/`. Schema:

```toml
id = "string-ohne-leerzeichen"
name = "Anzeigename"
description = "Wofür dieser Modus gedacht ist"
hotkey = "CommandOrControl+Alt+D"      # Accelerator-String
transcription = "local"                # "local" | "cloud"
processing = "none"                    # "none" | "local" | "cloud"
cloud_stt_provider = "xai"             # falls transcription = "cloud"
cloud_llm_provider = "xai"             # falls processing = "cloud"
cloud_llm_model = "grok-4"             # optional, sonst Provider-Default
local_llm_model = "qwen2.5:7b"         # falls processing = "local"
injection_method = "clipboard"         # "clipboard" | "keystrokes"
language = "de"                         # ISO-Code, Hinweis für STT
system_prompt = """
...mehrzeiliger Prompt...
"""
```

### 5.1 `exaktes_diktat.toml`
```toml
id = "exakt"
name = "Exaktes Diktat"
description = "Wörtliche Transkription, nur offensichtliche Erkennungsfehler korrigieren."
hotkey = "CommandOrControl+Alt+D"
transcription = "local"
processing = "none"
injection_method = "clipboard"
language = "de"
```

### 5.2 `korrigierendes_diktat.toml`
```toml
id = "korrektur"
name = "Korrigierendes Diktat"
hotkey = "CommandOrControl+Alt+K"
transcription = "local"
processing = "local"
local_llm_model = "qwen2.5:7b"
injection_method = "clipboard"
language = "de"
system_prompt = """
Du bekommst eine wörtliche Transkription gesprochener Sprache.
Aufgabe: Korrigiere Grammatik, Zeichensetzung und offensichtliche Versprecher.
Behalte den Inhalt, den Stil und die Aussage exakt bei. Lasse keine Information weg
und füge keine hinzu. Gib NUR den korrigierten Text aus, ohne Vorwort.
"""
```

### 5.3 `foermliche_email.toml`
```toml
id = "email"
name = "Förmliche E-Mail"
hotkey = "CommandOrControl+Alt+E"
transcription = "cloud"
processing = "cloud"
cloud_stt_provider = "xai"
cloud_llm_provider = "xai"
cloud_llm_model = "grok-4"
injection_method = "clipboard"
language = "de"
system_prompt = """
Du bekommst ein gesprochenes Diktat. Forme daraus eine förmliche E-Mail auf Deutsch.
- Verwende eine angemessene Anrede nur, wenn der Sprecher eine Empfängerperson erwähnt.
- Behalte alle inhaltlichen Aussagen vollständig bei.
- Korrigiere Grammatik, Stil und Struktur.
- Wenn der Sprecher eine Grußformel diktiert, übernimm sie. Sonst keine eigene hinzufügen.
- Gib AUSSCHLIESSLICH den E-Mail-Text aus, ohne erklärende Kommentare und ohne Markdown.
"""
```

### 5.4 `slack_teams.toml`
```toml
id = "chat"
name = "Slack/Teams Nachricht"
hotkey = "CommandOrControl+Alt+S"
transcription = "cloud"
processing = "cloud"
cloud_stt_provider = "xai"
cloud_llm_provider = "xai"
cloud_llm_model = "grok-4"
injection_method = "clipboard"
language = "de"
system_prompt = """
Du bekommst ein gesprochenes Diktat für eine Chat-Nachricht (Slack/Teams).
- Halte die Nachricht kurz, locker und direkt.
- Keine Anrede, keine Grußformel.
- Korrigiere Grammatik. Behalte den informellen Ton bei.
- Gib NUR den Nachrichtentext aus, ohne Kommentare.
"""
```

### 5.5 `github_issue.toml`
```toml
id = "issue"
name = "GitHub Issue"
hotkey = "CommandOrControl+Alt+G"
transcription = "cloud"
processing = "cloud"
cloud_stt_provider = "xai"
cloud_llm_provider = "xai"
cloud_llm_model = "grok-4"
injection_method = "clipboard"
language = "de"
system_prompt = """
Du bekommst ein gesprochenes Diktat. Forme daraus ein klares GitHub Issue in Markdown.
Struktur:
1. Knapper Titel als erste Zeile (ohne `#`-Markdown).
2. Leerzeile.
3. Abschnitt `## Problem` – was passiert, was wurde erwartet.
4. Abschnitt `## Reproduktion` – nur wenn der Sprecher Schritte nennt.
5. Abschnitt `## Zusatzinfos` – nur wenn relevant.
Keine Section weglassen, wenn der Sprecher Inhalt dafür liefert; keine erfinden.
Sprache: Deutsch, technisch sachlich. Gib NUR das Markdown aus.
"""
```

### 5.6 `claude_code_anweisung.toml`
```toml
id = "agent"
name = "Claude Code / Codex Anweisung"
hotkey = "CommandOrControl+Alt+C"
transcription = "cloud"
processing = "cloud"
cloud_stt_provider = "xai"
cloud_llm_provider = "xai"
cloud_llm_model = "grok-4"
injection_method = "clipboard"
language = "de"
system_prompt = """
Du bekommst ein gesprochenes Diktat, das eine Anweisung an einen Coding-Agenten
(Claude Code, Codex, etc.) werden soll.
- Forme die Anweisung in klare, präzise, imperativ formulierte Anforderungen um.
- Wenn der Sprecher mehrere Schritte nennt, nummeriere sie.
- Keine Höflichkeitsfloskeln, kein "bitte".
- Klar definierte Akzeptanzkriterien am Ende, falls aus dem Diktat ableitbar.
- Sprache: Deutsch oder Englisch — verwende die Sprache, in der diktiert wurde.
- Gib NUR die Anweisung aus.
"""
```

---

## 6. Phase 1 — DIESE AUFGABE

**Ziel:** Eine funktionierende Tray-App auf Windows + Linux/Wayland/X11, die per Hotkey aufnimmt, lokal transkribiert und das Ergebnis via Clipboard einfügt.

### 6.1 Definition of Done

- [ ] `pnpm tauri dev` startet die App auf Windows 10/11 und auf Linux (X11-Session) ohne Warnungen.
- [ ] Tray-Icon erscheint mit Kontextmenü: "Einstellungen öffnen", "Beenden". Icon ändert die Farbe über die vier States hinweg.
- [ ] Ein konfigurierbarer globaler Hotkey (Standard `Ctrl+Alt+D`) startet die Aufnahme; erneutes Drücken stoppt sie. Während der Aufnahme spielt ein dezenter Hinweiston bei Start und Stopp ab, und das Tray-Icon pulsiert rot.
- [ ] Audio wird vom Standard-Eingabegerät in einen 16-kHz-Mono-WAV-Buffer aufgenommen.
- [ ] Der Buffer wird lokal mit `whisper-rs` transkribiert. Default-Modell `ggml-large-v3-turbo-q5_0.bin` (siehe §2.1) wird beim ersten Start automatisch von Hugging Face heruntergeladen, mit SHA-Verifikation, in `app_data_dir()` abgelegt. Settings-UI bietet ein Modell-Dropdown (Default / Spar-Fallback / unquantisiert / eigener Pfad) und einen "Test-Transkription"-Button, der die tatsächliche RTF auf dem User-System misst.
- [ ] Der resultierende Text wird via Clipboard-Fallback-Pfad an der Cursor-Position eingefügt; der vorherige Clipboard-Inhalt wird innerhalb von 200 ms wiederhergestellt.
- [ ] Alle sechs Standard-Modi werden beim Start aus `modes/` geladen; einer davon (`exakt`) ist vollständig end-to-end verdrahtet. Die anderen fünf dürfen ihre Hotkeys registriert haben, loggen aber bei Auslösung "noch nicht implementiert".
- [ ] Settings-UI (React) zeigt: Liste der Audio-Geräte, aktuelle Modus-Liste (read-only ist für Phase 1 ausreichend), lokaler Whisper-Modellpfad, Hotkey-Anzeige.
- [ ] Fehler in jeder Stufe erscheinen als Desktop-Notification UND in einer Logs-Ansicht im UI. Keine stillen Fehlschläge.
- [ ] Cargo + ESLint + Prettier Configs sind eingerichtet. `cargo clippy -- -D warnings` und `pnpm lint` laufen sauber durch.
- [ ] Eine **GitLab-CI-Pipeline** (`.gitlab-ci.yml`) führt auf jedem Push aus: `cargo check`, `cargo clippy -- -D warnings`, `cargo test`, `pnpm lint`, `pnpm build` — auf Linux- *und* Windows-Runnern (Matrix-Jobs). Vollständiger `pnpm tauri build` inklusive Bundle-Erzeugung läuft **nur** auf Tags `v*`, nicht bei jedem Push (Kosten- und Zeit-Optimierung).
- [ ] **Windows-Verifikation der Phase-1-DoD ist explizit ein manueller End-to-End-Test durch den Maintainer auf Windows 10 oder 11**: Tray-Icon erscheint, Hotkey greift, Aufnahme + lokales Transkript + Clipboard-Inject funktionieren in mindestens einer Standard-App (z. B. Notepad oder einem Browser-Textfeld). CI-Pass alleine ist hierfür **nicht ausreichend**, weil CI keine UX-Verifikation leistet.

### 6.2 Außerhalb des Scope für Phase 1
- macOS-Implementierung (lasse die Rust-Implementierungs-Files als `unimplemented!()`-Stubs hinter `#[cfg(target_os = "macos")]`).
- Wayland (die Linux-Factory soll Wayland erkennen und einen klaren Fehler zurückgeben: "Wayland-Support kommt in Phase 5"; nicht abstürzen).
- Cloud-Provider (das Trait + die Modulstruktur existieren, inklusive `transcription/cloud/xai.rs`, `processing/cloud/openai_compatible.rs`, `processing/cloud/xai.rs`, `processing/cloud/openai.rs`, `processing/cloud/anthropic.rs`, aber nur die lokalen Implementierungen sind funktional; die Cloud-Module enthalten in Phase 1 nur Trait-konforme Stubs, die `unimplemented!()` zurückgeben).
- Streaming-STT (nur One-Shot in der Trait-Implementierung; siehe Abschnitt 4.7).
- Das Modus-Editor-UI (read-only Auflistung reicht).
- Auto-Update.
- Voice Activity Detection.

---

## 7. Arbeitsweise — Wie diese Aufgabe auszuführen ist

1. **Beginne damit, diese Datei vollständig zu lesen, sowie `docs/ARCHITECTURE.md` (das du als Teil von Phase 1 anlegen wirst).** Beginne nicht mit dem Programmieren, bevor du den Plan als nummerierte Checkliste konkreter Schritte zusammengefasst und auf meine Bestätigung gewartet hast.
2. Nach Bestätigung: Schritt für Schritt ausführen. Nach jedem größeren Schritt Build / Lint / Tests ausführen und Status berichten, bevor du weitermachst.
3. **Erfinde keine Features**, die nicht in diesem Dokument stehen. Wenn etwas mehrdeutig ist, frage nach, bevor du es codest.
4. **Ändere keine festgelegten Tech-Stack-Entscheidungen.** Wenn ein gewähltes Crate für unseren Use Case wirklich nicht funktioniert, halte an und frage.
5. **Committe pro logischem Schritt** mit Conventional Commits (`feat:`, `fix:`, `chore:`, `refactor:`, `docs:`, `test:`).
6. Halte übergreifende Entscheidungen in `docs/ARCHITECTURE.md` und plattformspezifische Eigenheiten in `docs/PLATFORMS.md` fest.
7. Aller Rust-Code: `cargo fmt` sauber, `cargo clippy -- -D warnings` sauber.
8. Aller TS-Code: Strict Mode an, kein `any`, ESLint sauber.
9. Tests, wo sie sinnvoll sind: reine Logik (Modus-Parsing, State-Machine-Übergänge, Prompt-Zusammenbau) bekommt Unit-Tests. Plattform-Code darf in Phase 1 manuell getestet bleiben.
10. **Lizenz: GPL-3.0-or-later.** Lege den vollen GPL-3.0-Lizenztext in `LICENSE` ab (von `https://www.gnu.org/licenses/gpl-3.0.txt` herunterladen). Lege ein `COPYING`-Symlink/-Kopie an. Füge den Standard-SPDX-Header in jede Rust-Quelldatei ein:
    ```rust
    // SPDX-License-Identifier: GPL-3.0-or-later
    ```
    Und in jede TypeScript-Quelldatei:
    ```ts
    // SPDX-License-Identifier: GPL-3.0-or-later
    ```
    Setze in `Cargo.toml` `license = "GPL-3.0-or-later"`. Setze in `package.json` `"license": "GPL-3.0-or-later"`. Füge im README einen "Lizenz"-Abschnitt hinzu, der GPL-3.0-or-later nennt und auf die LICENSE-Datei verlinkt. Prüfe, dass alle gewählten Rust-Crates und npm-Pakete GPL-3.0-kompatible Lizenzen haben — Apache-2.0, MIT, BSD, MPL-2.0, ISC sind alle in Ordnung; melde alles, was es nicht ist (z. B. CDDL, EPL, proprietär), bevor du es als Dependency hinzufügst.

---

## 8. Harte Einschränkungen (NICHT TUN)

- **Niemals** API-Keys im Klartext auf Disk speichern.
- **Niemals** Audio-Daten, Transkripte oder LLM-Antworten standardmäßig loggen. Es darf einen "Diagnose-Logging"-Toggle in den Einstellungen geben, standardmäßig aus, der das aktiviert.
- **Niemals** Telemetrie oder Analytics jeglicher Art einbauen.
- **Niemals** ein Whisper-Modell im Installer mitbündeln (Lizenz/Größe). Stattdessen einen Downloader bereitstellen, der es beim ersten Start aus der vom User gewählten Quelle holt.
- **Niemals** standardmäßig beim System-Start automatisch starten. Mache es zu einem expliziten Opt-in in den Einstellungen.
- **Niemals** `tauri-plugin-shell` benutzen, um externe Prozesse zu starten für Dinge, die wir in-process erledigen können.
- **Niemals** Fehler stillschweigend schlucken. Jedes `Result` wird entweder mit Kontext propagiert (`anyhow::Context`) oder mit einer für den User sichtbaren Notification behandelt.
- **Niemals** den xAI-API-Key in client-seitigem Code (Frontend) verwenden. Alle xAI-Requests, einschließlich des späteren WebSocket-Streamings, gehen durch das Rust-Backend.

---

## 9. Nach Phase 1

Sobald Phase 1 ausgeliefert und verifiziert ist, werden die nächsten Phasen als separate Aufgaben hinzugefügt:

- **Phase 2 — xAI end-to-end:** Ziel ist eine vollständig funktionierende Cloud-Pipeline mit nur einem API-Key (xAI). Reihenfolge:
  1. xAI STT (`transcription/cloud/xai.rs`) — One-Shot, multipart/form-data.
  2. Gemeinsamer `OpenAICompatibleClient` in `processing/cloud/openai_compatible.rs`.
  3. xAI LLM (`processing/cloud/xai.rs`) als Wrapper über (2).
  4. Settings-UI: API-Key-Eingabe für xAI mit "Verbindung testen" für STT und LLM separat.
  5. Die fünf bisher nicht verdrahteten Standard-Modi end-to-end fertigstellen.

- **Phase 2.5 — weitere Cloud-Provider:** OpenAI Whisper + GPT, Groq Whisper, Anthropic Claude, Deepgram. Reine Erweiterung, keine Architekturänderungen.

- **Phase 3:** Modus-Editor-UI; Per-Modus-Hotkeys mit Konflikterkennung; Onboarding-Wizard inklusive Eingabe und Test der API-Keys.

- **Phase 4 — Robustheit + Streaming:** Hot-Swap des Audio-Geräts, Umgang mit langen Aufnahmen, Retry-/Timeout-Logik, strukturierte Fehler-Taxonomie. **xAI WebSocket-Streaming-STT** als Live-Transkription-Modus (`transcription = "cloud_streaming"` als neuer Wert), Text erscheint während des Sprechens.

- **Phase 5:** Wayland-Support — `xdg-desktop-portal.GlobalShortcuts` für Hotkeys, `libei` über das `RemoteDesktop`-Portal für Tastendruck-Injection. Compositor-Kompatibilität dokumentieren (KDE Plasma, GNOME, Hyprland, Sway).

- **Phase 6:** macOS-Port + signierte Installer + Auto-Update.

Beginne keine dieser Phasen, bevor Phase 1 abgenommen ist.

---

## 10. Erste Aktion

Deine erste Aktion ist **nicht**, Code zu schreiben. Deine erste Aktion ist:

1. Bestätige, dass du diese Datei gelesen hast.
2. Erstelle einen Schritt-für-Schritt-Plan für Phase 1 als nummerierte Liste (10–25 Punkte).
3. Markiere jeden Punkt, bei dem du ein Risiko oder eine Mehrdeutigkeit siehst.
4. Warte auf meine Freigabe, bevor du das Repository scaffoldst.
