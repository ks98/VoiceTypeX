# VoiceTypeX — Architektur & Konventionen

> Lebende Referenz für die laufende Entwicklung. Auf Linux/Wayland (KDE
> Plasma 6) + Windows + X11 funktional komplett — Auto-Paste über
> `xdg-desktop-portal.RemoteDesktop` + libei. Settings und
> RemoteDesktop-Token persistent. Offen: macOS-Port + Distribution-
> Signing (Phase 6) und `wtype`-Fallback für Hyprland/Sway. Details
> in §11 „Roadmap".

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
  Beispiel: ein `tokio::select!` mit `mpsc_rx.recv()` ohne `if !eof`-Guard
  läuft nach Sender-Drop in eine Endless-Loop, weil `recv()` permanent
  `None` zurückliefert und der Branch dauerhaft gewinnt. „Mehr Timeout
  einbauen" wäre Symptom-Therapie; der Fix ist der Guard.
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
- `TextInjector` — `ClipboardFallbackInjector` für X11/Windows
  (enigo Ctrl+V), `WaylandLibeiInjector` für Wayland (libei via
  xdg-desktop-portal.RemoteDesktop, siehe §4.9). Auswahl in
  `make_default_injector` per Plattform-Detection.

Linux-Detection: `WAYLAND_DISPLAY` vs. `DISPLAY` zur Laufzeit.

### 4.3 Modus = TOML
Felder: `id`, `name`, `hotkey`, `transcription` (`local`/`cloud`),
`processing` (`none`/`local`/`cloud`), `cloud_stt_provider`,
`cloud_llm_provider`, `cloud_llm_model`, `language`, `system_prompt`,
`injection_method`. Repo-Defaults unter `modes/` werden beim ersten
Start nach `~/.config/.../modes/` kopiert und via `notify` heiß
nachgeladen.

### 4.4 BYOK + Secrets
Pro Provider ein Eintrag (Ausnahme: xAI = ein Eintrag für STT + LLM, weil
gleicher Key). Persistenz: JSON-File ist Source of Truth, OS-Keychain wird
best-effort parallel beschrieben — Linux-Setups mit gnome-keyring + kwallet
parallel sind unzuverlässig, daher diese Reihenfolge. Keys werden **nie**
geloggt, **nie** in Fehlerberichten serialisiert, **nie** ins Frontend
exponiert (alle Provider-Requests gehen durch das Rust-Backend).

**User-Settings** (PTT-Mode, Whisper-Slot, Audio-Gerät, Auto-Start, …)
liegen in `~/.config/.../settings.json` (kein Secret, chmod 0644).
`Settings::load_or_default(path)` beim App-Start, `Settings::save(path)`
nach jedem Mutations-IPC. Bei korruptem JSON Fallback auf Defaults
mit Log-Warning — App startet trotzdem sauber.

### 4.5 Clipboard als Default-Inject-Strategie
Save → Set → Paste → Restore (X11/Windows). Auf Wayland: nur Set + Notification
("Drücke Ctrl+V"). Direkte Tastendruck-Injection (`injection_method =
"keystrokes"`) ist Opt-in pro Modus, nützlich für Exaktes-Diktat.

### 4.6 Cloud-Provider-Trennung — STT vs. LLM
**LLM:** alle OpenAI-Chat-Completions-kompatiblen Provider teilen einen
`OpenAICompatibleClient` (Base-URL + Default-Model). Anthropic ist eigenständig
(Messages-API).
**STT:** kein gemeinsamer Wrapper — xAI hat eigenes API (multipart);
OpenAI und Groq sind Whisper-kompatibel; Deepgram eigenständig. Keine
künstliche Gemeinsamkeit konstruieren, die real nicht existiert.

### 4.7 Pipeline-Events ans Frontend
StateBus-Übergänge werden als Tauri-Event `app://state` ans Frontend
emittiert (Payload: `{state: "recording"|"transcribing"|...}`). Das
Overlay-Window abonniert das Event und rendert phasengerechte Status-
Texte (siehe §4.8). Damit hängt das Overlay an genau einer kanonischen
Datenquelle — neue Auslöse-Pfade (Hotkey, UI-Button, IPC) ändern den
State, das Overlay folgt automatisch.

### 4.8 Live-Overlay-Window
Zweites Tauri-Window (`label: "overlay"`, transparent, `alwaysOnTop`,
`skipTaskbar`, ohne Decorations, `focus: false`), oben links 24 px vom
Rand entfernt, 520 × 96 px. Zeigt während einer Aufnahme einen
pulsierenden roten Punkt und einen Phase-Text (*„Höre zu …"* /
*„Transkribiere …"* / *„Verarbeite …"* / *„Füge ein …"*). Window-Routing
zwischen Haupt-Fenster und Overlay über URL-Query-Parameter
(`?window=overlay`) in `src/main.tsx`.

**Pflicht-Disziplinen für Wayland-Fokus-Neutralität:**

1. Das Overlay-Window ist initial **versteckt** (`visible: false`).
   Sichtbarkeit wird **vom Backend** gesteuert: `start_recording`
   ruft `overlay.show()`, **direkt vor dem libei-Inject** ruft die
   Pipeline `overlay.hide()` (mit 80 ms Pause), damit der Tastatur-Fokus
   zur Ziel-App zurückspringt. State-Listener versteckt das Overlay
   außerdem bei `Idle`/`Error` als Cleanup-Pfad.
2. CSS `pointer-events-none` auf dem Overlay-Root-Element, damit Klicks
   während der kurzen Sichtbarkeitsphase nicht abgefangen werden.

**Warum nicht `visible: true` mit Opacity-Toggle?** Diese Variante haben
wir probiert — KWin optimiert ein dauerhaft transparentes + nicht-
interaktives Window weg, beim ersten Sichtbarmachen ist der Frame nicht
aktiv. Der `opacity: 0.001`-Hack flackerte beim App-Start. Die jetzige
Lösung (echtes `show()` / `hide()` mit explizitem Hide-vor-Inject) ist
robust: das Overlay ist außer während Aufnahme/Transkription/Postproc
konsequent unsichtbar, und der Fokus-Klau-Effekt durch `show()` wird
durch das vor-dem-Inject `hide()` neutralisiert.

**Warum nicht zusätzlich `set_ignore_cursor_events(true)`?** Das war in
einer Zwischen-Iteration drin (für die Opacity-Toggle-Variante mit
dauerhaft sichtbarem Window). Auf einem initial-hidden Window triggert
der Aufruf einen tao-Panic in der GTK-EventLoop (`Option::unwrap() on
None` in `tao::platform_impl::linux::event_loop:457`). Mit dem
Backend-show/hide-Pattern reicht `pointer-events-none` als CSS-Schutz
vollkommen.

**Hauptfenster startet versteckt** (`visible: false`). Erreichbar über
Tray-Linksklick oder „Einstellungen öffnen". X-Knopf versteckt nur,
beendet nicht — User kann es jederzeit wiederbringen. Der Grund:
analog zum Overlay-Fokus-Klau-Problem würde bei sichtbarem Hauptfenster
KDE Plasma's xdg-portal beim Trigger des globalen Shortcuts das
App-Fenster fokussieren und damit den User-Fokus auf der Ziel-App
klauen; libei-Strg+V landet dann im VoiceTypeX-Hauptfenster statt in
der Ziel-App. Versteckt = nichts zu fokussieren.

### 4.9 Wayland Auto-Paste via libei
Auf Wayland kann eine App ohne Compositor-Hilfe keine Tastendrücke
synthetisieren — `xdotool`/XTest sind X11-only, das ist Sicherheitsmodell,
kein Bug. Lösung: `xdg-desktop-portal.RemoteDesktop` + libei.

**Stack:** `ashpd 0.11` für Portal-Session, `reis 0.6.1` (gepinnt — API
vor 1.0) für das EI-Protokoll, `xkbcommon` für Layout-bewusstes
Keysym→Keycode-Mapping. Vorbild für die Implementierung ist
[lan-mouse](https://github.com/feschber/lan-mouse).

**Pflicht-Disziplinen aus der EI-Spec** (libinput/libei `protocol.xml`),
ohne die Tastendrücke silent verworfen werden:

1. **`start_emulating` exklusiv im `ei_device::Resumed`-Handler** rufen,
   nicht direkt nach `Device::Done`. Spec wörtlich: *„client bug to
   request emulation on a device that is not resumed; the EIS
   implementation may silently discard such events."*
2. **`sequence`-Counter strikt monoton** über die App-Lebensdauer.
3. **`device.frame(serial, time)`-`time` strikt monoton** (Mikrosekunden).
   Wir leiten aus `Instant::now()` ab — Rust's `Instant` ist
   spec-monoton.
4. **Persistente Session statt pro-Strg+V neu öffnen** (Stop-Cycles
   sind erlaubt, aber bei `persist_mode != Permanent` würde jeder Inject
   einen Permission-Dialog auslösen).
5. **`Paused`-Event setzt `emulation_active` zurück** — beim nächsten
   `Resumed` muss erneut `start_emulating` mit incrementiertem `sequence`
   aufgerufen werden.

**Architektur:** `WaylandLibeiInjector` im Tauri-Hauptthread (tokio),
Worker-Thread (sync, manuelle Poll-Loop mit `set_nonblocking(true)` auf
dem EIS-FD) für den EI-Protokoll-Handshake. Brücke ist ein
`std::sync::mpsc::Sender<KeyCommand>` aus tokio in den Worker; Setup-
Status via `tokio::oneshot` zurück. **Composite-Strategie:** Clipboard
für den Text-Transport, libei nur für den `Ctrl+V`-Keystroke.

**Compositor-Matrix** (Mai 2026, in Praxis verifiziert): KDE Plasma 6.1+
und GNOME 46+/47 funktionieren über das Portal; Hyprland und Sway/wlroots
brauchen einen `wtype`-Sub-Prozess-Fallback (siehe §11). Mindestversionen:
`xdg-desktop-portal ≥ 1.18`, `libei ≥ 1.0`.

**Failure-UX:** Wenn der User den Permission-Dialog ablehnt oder die
Session aus anderen Gründen scheitert, fällt der Injector silent auf
Clipboard + Notification *„Drücke Strg+V"* zurück. Kein harter Fehler.

**`restore_token`-Persistenz:** Der vom Compositor zurückgegebene
`restore_token` wird in `~/.config/.../wayland_session.json` (chmod
0600) gespeichert und bei nächsten App-Starts in `select_devices`
durchgereicht. Damit kommt der Permission-Dialog **nur einmal**, beim
allerersten Inject. Bei Token-Reject (z.B. nach Compositor-Neustart)
fällt das Setup auf den normalen Permission-Flow zurück und schreibt
einen frischen Token.

---

## 5. Verifizierte Provider-Protokolle

> Quelle: docs.x.ai (Stand April 2026). Bei API-Änderungen vor Implementierung
> mit WebFetch verifizieren — nicht auf diese Tabelle als Zukunfts-Garantie
> verlassen.

### 5.1 xAI STT
`POST https://api.x.ai/v1/stt`, multipart/form-data mit `file` als **letztem**
Feld, Auth via Bearer-Header. Response: `{text, language, duration, words[]}`.
Wir nutzen nur `text`. xAI's `language`-Parameter ist nur Text-Formatting
(Zahlen/Währungen), keine Sprach-Erzwingung — die Erkennung ist auto-detect.

### 5.2 xAI Grok (Cloud-LLM)
OpenAI-Chat-Completions-kompatibel. **Default-Model `grok-4-fast-non-reasoning`**
für Postprocessing (kein Reasoning-Overhead, 2 M Context, ~6 × günstiger als
`grok-4`). `grok-4` nur opt-in pro Modus, wenn echtes Multi-Step-Reasoning
gebraucht wird.

### 5.3 OpenAI / Anthropic / Groq / Deepgram
Standard-APIs. Implementierungen unter `src-tauri/src/processing/cloud/` und
`src-tauri/src/transcription/cloud/`.

---

## 6. Standard-Modi

Sechs Modi werden beim ersten Start aus dem Repo-Verzeichnis `modes/` nach
`~/.config/.../modes/` kopiert:

| Modus | Hotkey | STT | LLM-Postproc |
|---|---|---|---|
| Exaktes Diktat | `Ctrl+Alt+D` | Lokal | — |
| Korrigierendes Diktat | `Ctrl+Alt+K` | Lokal | Lokal (Ollama) |
| Förmliche E-Mail | `Ctrl+Alt+E` | xAI | xAI Grok-fast |
| Slack/Teams | `Ctrl+Alt+S` | xAI | xAI Grok-fast |
| GitHub-Issue | `Ctrl+Alt+G` | xAI | xAI Grok-fast |
| Claude-Code-Anweisung | `Ctrl+Alt+C` | xAI | xAI Grok-fast |

User können eigene Modi via TOML hinzufügen — keine Code-Änderung nötig.

---

## 7. Plattform-Status

| Plattform | Hotkey | Audio | Transkription | Auto-Paste | Tray |
|---|---|---|---|---|---|
| Linux X11 | ✅ tauri-plugin-global-shortcut | ✅ | ✅ | ✅ enigo (XTest) | ✅ |
| Linux Wayland | ✅ xdg-portal | ✅ | ✅ | ✅ libei (KDE Plasma 6.1+, GNOME 46+); Hyprland/Sway noch offen | ✅ |
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
- **Niemals** `#[derive(Default)]` als Ersatz für serde-`#[serde(default = "...")]`
  benutzen — die beiden Mechanismen sehen einander nicht. Wenn ein Feld einen
  echten Anwendungs-Default braucht, manueller `impl Default`.
- **Niemals** auf Wayland (KDE Plasma) ein `alwaysOnTop`-Window per
  `show()` zeigen und libei-Inject *währenddessen* ausführen — `show()`
  klaut den Tastatur-Fokus, libei-Events landen im Overlay statt der
  Ziel-App. Stattdessen: vor dem libei-Inject explizit `overlay.hide()`
  + 80 ms Pause, damit Fokus zurückspringt (siehe §4.8).
- **Niemals** `webview_window.set_ignore_cursor_events(true)` auf einem
  initial-hidden Window (`visible: false`) aufrufen — triggert
  tao-Panic in der Linux-GTK-EventLoop. Wenn pointer-passthrough nötig
  ist, das Window erst zeigen und *dann* den Aufruf machen — oder per
  CSS `pointer-events-none` lösen (das ist ohnehin robuster und
  plattformneutral).

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

### Phase 6 — macOS + Distribution-Hardening
- macOS-Implementierungen (CGEvent für Inject, NSStatusItem für Tray).
- Signierte Installer (Apple Notarization, Windows Authenticode).
- Auto-Update via tauri-plugin-updater mit signierten Manifesten.

### Optional / nice-to-have
- **`wtype`-Fallback** für Hyprland und Sway/wlroots (Compositors ohne
  `xdg-desktop-portal.RemoteDesktop`-Support). Detection via
  D-Bus-Introspection auf `ConnectToEIS`; bei Nicht-Verfügbarkeit
  Sub-Prozess-Aufruf von `wtype`. Nicht-blockierend für KDE Plasma 6 /
  GNOME 46+ User.

### Bekannte Limitierungen ohne geplanten Fix
- xAI's STT-API hat keine Sprach-Erzwingung — die Erkennung ist
  hartcodiert auto-detect. Bei kurzen, sprachneutralen Diktaten kann das
  Modell daneben raten. Workaround wäre Fallback auf lokales whisper-rs
  mit erzwungenem `language=de`; aktuell akzeptieren wir das Limit.
