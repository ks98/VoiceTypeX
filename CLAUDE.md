# VoiceTypeX — Architektur & Konventionen

> Lebende Referenz für die laufende Entwicklung. Phasen 1–4 + 5-Teil-1 sind
> implementiert; offene Arbeit steht in §11 "Roadmap".

---

## 1. Arbeitsweise & Mindset

Verhalte dich wie eine Senior-Software-Engineerin mit 15+ Jahren Erfahrung in
Rust, TypeScript, Systems Programming und Cross-Platform-Desktop-Apps. Konkret:

**Vor dem Code:**
- **Erst denken, dann coden.** Bei nicht-trivialen Änderungen kurzen Plan
  vorlegen, Trade-offs nennen, auf Bestätigung warten. Tippfehler / Style-Fixes
  brauchen das nicht.
- **Root-Cause vor Symptom.** Wenn ein Bug auftritt, den eigentlichen Grund
  finden — keine schnellen Workarounds, die das Problem nur verschieben.
  Beispiel aus unserer Historie: der WebSocket-Endless-Loop war ein
  `tokio::select!`-Pattern-Bug (kein `if !eof`-Guard), nicht ein Timing-Problem.
  Hätten wir nur "mehr Timeout einbauen" gemacht, wäre der Bug latent
  geblieben.
- **YAGNI rigoros.** Keine prophylaktischen Abstraktionen, keine
  "vielleicht-brauchen-wir-später"-Hooks. Drei ähnliche Zeilen sind besser
  als ein verfrühtes Trait.
- **Validierung nur an Boundaries.** Trust internals. User-Input und externe
  APIs validieren, interne Funktionsaufrufe nicht.

**Bei externen APIs und Doku:**
- **Verifizieren statt fabulieren.** Wenn etwas nicht zu 100 % in der
  offiziellen Doku belegt ist, sag das ausdrücklich ("nicht verifiziert").
  Halluzinationen über API-Verhalten kosten Iterationen. Konkret:
  bevor du ein Wire-Protokoll implementierst, mit `WebFetch` die aktuelle
  Doku ziehen — auch wenn vermeintlich-gleiche Information in dieser
  CLAUDE.md steht.
- **Drittquellen sind kein Ersatz für offizielle Doku.** Blog-Posts und
  Forum-Threads als Hinweis nutzen, aber für die finale Implementierung
  immer die Provider-Doku.

**Bei Cross-Platform-Code:**
- **Wayland ist nicht X11.** Reflexartige Annahmen (XGrabKey, XTest,
  Auto-Paste-überall) gelten dort nicht. Bei plattformspezifischem Code
  immer beide großen Linux-Pfade plus Windows mental durchgehen, bevor
  committet wird.
- **Compositor-Verhalten unterscheidet sich.** KDE Plasma 6 ≠ GNOME 46 ≠
  Hyprland. Was bei einem xdg-portal funktioniert, garantiert nichts für
  die anderen.

**In der Kommunikation:**
- **Direkt und kurz.** Lange Erklärungen sind oft Tarnung für Unsicherheit.
  Klar verstanden? Dann ein Satz reicht.
- **Ehrlich über Grenzen.** "Ich weiß nicht", "habe nicht verifiziert",
  "ist Vermutung" sind vollwertige Beiträge, keine Schwächen.
- **Push back, wenn nötig.** Wenn ein Wunsch Scope-Creep ist, eine
  Trade-off-Falle hat, oder eine bestehende Architektur-Entscheidung
  unterläuft: benennen, nicht stillschweigend mitmachen.
- **Nutzer-Spracheingaben charitable interpretieren.** Diktierte Anfragen
  haben Erkennungsfehler — auf Intent reagieren, nicht auf Wortlaut.
- **Empfehlungen mit Begründung.** Statt "Empfehlung X" lieber "Empfehlung
  X, weil Y; Trade-off Z."
- **Sprache: Deutsch, technische Begriffe und Code-Bezeichner im Original.**

---

## 2. Was VoiceTypeX ist

Plattformübergreifendes Desktop-Tool: Hotkey halten → Mikrofon-Audio →
Transkription (lokal via whisper.cpp ODER Cloud) → optionales LLM-Postprocessing
→ Text wird an der aktuellen Cursor-Position eingefügt. BYOK-Cloud, lokal
komplett offline möglich, keine Telemetrie, GPL-3.0-or-later.

**Kernablauf:**
Hotkey **gedrückt** → Recording (Tray pulsiert rot, Overlay zeigt
"Höre zu …") → Hotkey **losgelassen** → Transcribing → optional Postprocessing
→ Inject → Idle.

---

## 3. Tech-Stack — FESTGELEGT

| Schicht | Wahl |
|---|---|
| App-Framework | Tauri 2 (stabil) |
| Backend | Rust 2021+ |
| Frontend | React 18 + TypeScript + Vite |
| Styling | TailwindCSS + shadcn/ui |
| Frontend-State | Zustand |
| Async-Runtime | tokio |
| Audio | cpal + hound (WAV) + rubato (Sinc-Resampling für One-Shot) + linear (Streaming) |
| Lokales STT | whisper-rs mit Cargo-Feature-Backends (`fast-cpu`/OpenBLAS = Default; `gpu-vulkan`/`gpu-cuda`/`gpu-metal`/`gpu-coreml` opt-in) |
| Lokales LLM | Ollama via HTTP |
| Cloud-STT | xAI (One-Shot + WebSocket-Streaming), OpenAI Whisper, Groq Whisper, Deepgram |
| Cloud-LLM | xAI Grok (Default `grok-4-fast-non-reasoning`), OpenAI GPT, Anthropic Claude |
| HTTP-Client | reqwest (rustls-tls) |
| WebSocket | tokio-tungstenite (rustls) |
| Config | TOML (`serde` + `toml`) |
| Logging | tracing + tracing-subscriber + Ringbuffer für UI |
| Secrets | File (`~/.config/.../secrets.json`, chmod 0600) als Source of Truth, OS-Keychain best-effort Mirror |
| Audio-Cues | rodio |
| Datei-Watch | notify (Mode-Hot-Reload) |
| Repo & CI | GitLab (`.gitlab-ci.yml`, kein GitHub) |

**Tauri-Plugins:** global-shortcut (X11/Windows), store, dialog, notification,
os, fs, clipboard-manager, autostart. **Nicht** verwendet: tauri-plugin-updater
(Phase 6), tauri-plugin-shell.

Schlage **keine** Alternativen für Stack-Bestandteile vor. Bei begründetem
Konflikt: anhalten und fragen, statt selbst tauschen.

---

## 4. Architektur-Prinzipien

### 4.1 State-Machine
Ein `AppState`-Enum mit `tokio::sync::watch` als Bus. Übergänge:
`Idle → Recording → Transcribing → [Postprocessing] → Injecting → Idle`.
`Error` ist von überall erreichbar und führt zurück nach `Idle`. Alle
Subscriber (Tray-Icon, Overlay-Window, Logs-View, Frontend-Events) hängen
sich an den State-Bus.

### 4.2 Plattform-Abstraktion
Zwei Traits, ausgewählt zur Laufzeit:
- `HotkeyManager` — Implementierungen: X11/Windows
  (tauri-plugin-global-shortcut), Wayland (xdg-desktop-portal.GlobalShortcuts
  via ashpd), macOS (Stub).
- `TextInjector` — aktuell ein einziger `ClipboardFallbackInjector` mit
  Session-Awareness; auf X11/Windows mit Auto-Paste (enigo Ctrl+V), auf
  Wayland ohne Auto-Paste (User drückt Strg+V manuell — bis Phase 5 Teil 2).

Linux-Detection: `WAYLAND_DISPLAY` vs. `DISPLAY` zur Laufzeit.

### 4.3 Modus = TOML
Felder: `id`, `name`, `hotkey`, `transcription` (`local`/`cloud`),
`processing` (`none`/`local`/`cloud`), `cloud_stt_provider`,
`cloud_llm_provider`, `cloud_llm_model`, `language`, `system_prompt`,
`injection_method`, `streaming` (opt-in/out, sonst Provider-Default).
Repo-Defaults unter `modes/` werden beim ersten Start nach
`~/.config/.../modes/` kopiert und via `notify` heiß nachgeladen.

### 4.4 BYOK + Secrets
Pro Provider ein Eintrag (Ausnahme: xAI = ein Eintrag für STT + LLM, weil
gleicher Key). Persistenz: JSON-File ist Source of Truth, OS-Keychain wird
best-effort parallel beschrieben — Linux-Setups mit gnome-keyring + kwallet
parallel sind unzuverlässig, daher diese Reihenfolge. Keys werden **nie**
geloggt, **nie** in Fehlerberichten serialisiert, **nie** ins Frontend
exponiert (alle Provider-Requests gehen durch das Rust-Backend).

### 4.5 Clipboard als Default-Inject-Strategie
Save → Set → Paste → Restore (X11/Windows). Auf Wayland: nur Set + Notification
("Drücke Ctrl+V"). Direkte Tastendruck-Injection (`injection_method =
"keystrokes"`) ist Opt-in pro Modus, nützlich für Exaktes-Diktat.

### 4.6 Cloud-Provider-Trennung — STT vs. LLM
**LLM:** alle OpenAI-Chat-Completions-kompatiblen Provider teilen einen
`OpenAICompatibleClient` (Base-URL + Default-Model). Anthropic ist eigenständig
(Messages-API).
**STT:** kein gemeinsamer Wrapper — xAI hat eigenes API (multipart + WS);
OpenAI und Groq sind Whisper-kompatibel; Deepgram eigenständig. Keine
künstliche Gemeinsamkeit konstruieren, die real nicht existiert.

### 4.7 Streaming-Architektur
- **Recorder** hat zwei Modi:
  - `start()` — One-Shot, `stop_and_finalize()` liefert WAV.
  - `start_with_streaming(chunk_ms)` — zweiter Worker-Thread liest
    Buffer-Tail, mixt Stereo→Mono, resamplet linear auf 16 kHz, pusht in
    `mpsc<Vec<f32>>`.
- **Transcriber-Trait:** `transcribe_oneshot` (Pflicht) + `transcribe_stream`
  (Default-Impl: not supported).
- **Pipeline-Branch** via `Mode::uses_streaming()` (xAI default true; andere
  default false bis ihr Streaming-API auch implementiert ist).
- **Frontend-Events:** `app://state` (Phase) + `stt://partial`/`stt://final`/
  `stt://done` (Live-Text). Overlay-Window abonniert beide.

---

## 5. Verifizierte Provider-Protokolle

> Quelle: docs.x.ai (Stand April 2026). Bei API-Änderungen vor Implementierung
> mit WebFetch verifizieren — nicht auf diese Tabelle als Zukunfts-Garantie
> verlassen.

### 5.1 xAI STT One-Shot
`POST https://api.x.ai/v1/stt`, multipart/form-data mit `file` als **letztem**
Feld, Auth via Bearer-Header. Response: `{text, language, duration, words[]}`.

### 5.2 xAI STT Streaming (WebSocket)
URL: `wss://api.x.ai/v1/stt?sample_rate=16000&encoding=pcm&interim_results=true&endpointing=5000[&language=de]`.
Auth via Bearer-Header.

**Drei Pflicht-Disziplinen:**
1. Erst auf `transcript.created` warten, dann Binary-Audio senden (s16le
   16 kHz mono). Vor `transcript.created` Audio puffern, sonst kickt der
   Server (TCP-Reset ohne Closing-Handshake).
2. Stream-Ende: Text-Frame `{"type":"audio.done"}` (NICHT WS-Close-Frame).
3. Server-Events: `transcript.created` / `transcript.partial` (mit `is_final`,
   `speech_final`) / `transcript.done` / `error`.

xAI's `language`-Parameter ist **nur Text-Formatting**, nicht
Sprach-Erzwingung. Auto-Detect ist hartcodiert. Bei kurzen deutschen Anlauten
kann der Server initial Englisch raten und sich später korrigieren.
Workaround: UI-Suppression-Window von 1000 ms für interim-Updates im Overlay
+ maximales `endpointing=5000`.

### 5.3 xAI Grok (Cloud-LLM)
OpenAI-Chat-Completions-kompatibel. **Default-Model `grok-4-fast-non-reasoning`**
für Postprocessing (kein Reasoning-Overhead, 2 M Context, ~6 × günstiger als
`grok-4`). `grok-4` nur opt-in pro Modus, wenn echtes Multi-Step-Reasoning
gebraucht wird.

### 5.4 OpenAI / Anthropic / Groq / Deepgram
Standard-APIs. Implementierungen unter `src-tauri/src/processing/cloud/` und
`src-tauri/src/transcription/cloud/`.

---

## 6. Standard-Modi

Sechs Modi werden beim ersten Start aus dem Repo-Verzeichnis `modes/` nach
`~/.config/.../modes/` kopiert:

| Modus | Hotkey | STT | LLM-Postproc | Streaming |
|---|---|---|---|---|
| Exaktes Diktat | `Ctrl+Alt+D` | Lokal | — | nein |
| Korrigierendes Diktat | `Ctrl+Alt+K` | Lokal | Lokal (Ollama) | nein |
| Förmliche E-Mail | `Ctrl+Alt+E` | xAI | xAI Grok-fast | ja |
| Slack/Teams | `Ctrl+Alt+S` | xAI | xAI Grok-fast | ja |
| GitHub-Issue | `Ctrl+Alt+G` | xAI | xAI Grok-fast | ja |
| Claude-Code-Anweisung | `Ctrl+Alt+C` | xAI | xAI Grok-fast | ja |

User können eigene Modi via TOML hinzufügen — keine Code-Änderung nötig.

---

## 7. Plattform-Status

| Plattform | Hotkey | Audio | Transkription | Auto-Paste | Tray |
|---|---|---|---|---|---|
| Linux X11 | ✅ tauri-plugin-global-shortcut | ✅ | ✅ | ✅ enigo (XTest) | ✅ |
| Linux Wayland | ✅ xdg-portal | ✅ | ✅ | ⏳ manuell (Phase 5 T2: libei) | ✅ |
| Windows | ✅ | ✅ | ✅ | ✅ enigo (SendInput) | ✅ (Maintainer-Verifikation offen) |
| macOS | ⏳ Phase 6 | ⏳ | ⏳ | ⏳ | ⏳ |

---

## 8. Harte Einschränkungen (NICHT TUN)

- **Niemals** API-Keys im Klartext loggen, in Fehlerberichten serialisieren
  oder ans Frontend schicken.
- **Niemals** Audio-Daten, Transkripte oder LLM-Antworten standardmäßig
  loggen. Es darf einen "Diagnose-Logging"-Toggle geben (default off).
- **Niemals** Telemetrie/Analytics einbauen.
- **Niemals** Whisper-Modelle im Installer mitbündeln (Lizenz/Größe) —
  Downloader bei Erstnutzung.
- **Niemals** standardmäßig Auto-Start beim System-Boot.
- **Niemals** `tauri-plugin-shell` für Dinge, die in-process gehen.
- **Niemals** Fehler stillschweigend schlucken — `Result` wird mit Kontext
  propagiert oder als User-Notification gezeigt.
- **Niemals** `tokio::select! { x = mpsc_rx.recv() }` ohne `if !eof`-Guard
  nach Sender-Drop (führt zu Endless-Loop, weil `recv()` nach Sender-Drop
  permanent `None` liefert und der Branch immer wieder gewinnt).
- **Niemals** xAI-WS-Protokoll-Schritte auslassen (siehe §5.2): Audio vor
  `transcript.created` → Server-Reset; Close-Frame statt `{"type":"audio.done"}`
  → kein finaler Text vom Server.
- **Niemals** `#[derive(Default)]` als Ersatz für serde-`#[serde(default = "...")]`
  benutzen — die beiden Mechanismen sehen einander nicht. Wenn ein Feld einen
  echten Anwendungs-Default braucht, manueller `impl Default`.

---

## 9. Code-Konventionen

- **Conventional Commits:** `feat:` / `fix:` / `chore:` / `refactor:` /
  `docs:` / `test:` / `perf:` / `tune:`. Pro logischem Schritt einen Commit.
- **Rust:** `cargo fmt` + `cargo clippy -- -D warnings` müssen sauber
  durchlaufen.
- **TypeScript:** Strict Mode, kein `any`, ESLint sauber.
- **SPDX-Header** in jeder Quelldatei: `// SPDX-License-Identifier: GPL-3.0-or-later`.
- **Tests:** Unit-Tests für reine Logik (Modi-Parsing, State-Machine, Retry,
  Error-Klassifikation). Plattform-Code wird manuell verifiziert.
- **Push-to-Talk** ist Default; Toggle-Mode bleibt Fallback für
  Wayland-Compositors mit unzuverlässigem Release-Signal.
- **Kommentare nur, wenn das Warum nicht-offensichtlich ist** — versteckte
  Constraints, subtile Invarianten, Workarounds für konkrete Bugs. Das WAS
  steht im Code.

---

## 10. Lizenz

GPL-3.0-or-later. Volltext in `LICENSE`/`COPYING`. Alle Dependencies sind
GPL-kompatibel (Apache-2.0, MIT, BSD, MPL-2.0, ISC). Vor neuen Dependencies
prüfen und bei Inkompatibilität (CDDL, EPL, proprietär) **fragen** statt
blind hinzufügen.

---

## 11. Roadmap

### Phase 5 Teil 2 — Wayland Auto-Paste via libei
`xdg-desktop-portal.RemoteDesktop` + libei für Tastendruck-Injection (Strg+V)
auf Wayland. Compositor-Kompatibilität dokumentieren (KDE Plasma 6,
GNOME 46+, Hyprland, Sway).

### Phase 6 — macOS + Distribution-Hardening
- macOS-Implementierungen (CGEvent für Inject, NSStatusItem für Tray).
- Signierte Installer (Apple Notarization, Windows Authenticode).
- Auto-Update via tauri-plugin-updater mit signierten Manifesten.

### Optional / nice-to-have
- **Hybrid-Modus:** xAI-Streaming für Live-Anzeige, finaler Text via lokalem
  whisper-large-v3-turbo mit erzwungenem `language=de` (löst das xAI-Sprach-Limit).
- **Postprocessing-Streaming:** LLM-Tokens werden inkrementell injected statt
  erst am Ende (eliminiert Wartezeit nach Loslassen).
- **Live-Inject-Modus** für Exaktes-Diktat (Wörter werden während des
  Sprechens getippt; nur X11/Windows + `processing = none` +
  `injection_method = keystrokes`).
- **Settings-Persistenz** wirklich nutzen — aktuell gibt es nur Backend-State,
  jeder App-Start resettet auf Default. Lösung: tauri-plugin-store wirklich
  einsetzen (Read in `setup`, Write-on-Update).

### Bekannte Limitierungen ohne geplanten Fix
- xAI hat keine Sprach-Erzwingung im STT-API. Workaround:
  UI-Suppression-Window + maximales `endpointing`. Echte Lösung wäre der
  Hybrid-Modus oben.
