# VoiceTypeX

Plattformübergreifendes Desktop-Werkzeug, das Diktate in Text verwandelt
und ihn an der aktuellen Cursor-Position einfügt — überall, in jeder App.
Ein globaler Hotkey öffnet ein Modus-Menü, Pfeile + Enter wählen, Diktat
läuft — derselbe Hotkey stoppt.

## Status

Funktional komplett auf **Linux/Wayland (KDE Plasma 6 + GNOME 46+)**,
**Linux/X11** und **Windows**. Auto-Paste auf Wayland über
`xdg-desktop-portal.RemoteDesktop` + libei. Settings und Wayland-
Permission-Token persistent. Distribution-Bundles für Linux
(`.deb` / `.rpm` / AppImage) + Windows (NSIS) verfügbar. macOS ist
nicht im Scope.

## Kernablauf

1. Globaler **Menü-Hotkey** drücken (Default `Ctrl+Alt+Space`) → das
   Overlay zeigt die Modus-Liste, der Cursor steht auf der zuletzt
   getroffenen Auswahl.
2. Mit `↑`/`↓` einen anderen Modus wählen (oder direkt `Enter`, um die
   zuletzt verwendete Auswahl zu bestätigen). `Esc` schließt das Menü
   ohne Aktion.
3. Nach `Enter` startet die Aufnahme, Tray-Icon pulsiert rot, Overlay
   zeigt *„Höre zu …"*. Sprich.
4. **Denselben Hotkey nochmal drücken** → Audio wird transkribiert
   (lokal via whisper.cpp **oder** Cloud-STT), optional durch ein LLM
   nach dem gewählten **Modus** nachbearbeitet (lokal via Ollama
   **oder** Cloud-LLM), und an der Cursor-Position eingefügt.

Sechs Modi sind vorinstalliert: *Exaktes Diktat*, *Korrigierendes
Diktat*, *Förmliche E-Mail*, *Slack/Teams Nachricht*, *GitHub Issue*,
*Anweisung an Coding-Agent*. Eigene Modi via TOML — siehe
[`docs/MODES.md`](docs/MODES.md).

**Migration aus älteren Versionen:** Modi hatten bisher je einen eigenen
Hotkey (`Ctrl+Alt+D`, `Ctrl+Alt+E`, …). Der Wechsel auf den einzigen
Menü-Hotkey ist transparent — bestehende `hotkey`-Felder in
`modes/*.toml` werden weiterhin akzeptiert, aber ignoriert. Den
Menü-Hotkey selbst änderst du in *Einstellungen* → *Globaler
Menü-Hotkey*.

## Tech-Stack (kompakt)

- **App-Framework:** Tauri 2 (stable)
- **Backend:** Rust 2021+ mit tokio
- **Frontend:** React 18 + TypeScript strict + Vite + TailwindCSS +
  shadcn/ui + Zustand
- **Internationalisierung:** eigener `useT()`-Hook (~70 LOC, kein i18next)
  mit `Intl.PluralRules`. Vollstaendig in `de`, `en`, `fr`, `es`, `it`
  ausgeliefert: alle UI-Strings (~400), Tray-Menue, per-Locale Default-
  Modi mit kulturell angepassten `system_prompt`s. OS-Locale-Detection
  im Backend (`tauri_plugin_os::locale()`), Live-Switch via Settings
  ohne App-Neustart. Build-Gate `pnpm i18n:check` validiert Locale-
  Parity und Used-Key-Existenz.
- **Audio:** cpal + hound (WAV) + rubato (Sinc-Resampling auf 16 kHz)
- **Lokales STT:** whisper-rs 0.16 mit Silero-VAD v6 (verhindert
  Stille-Halluzinationen). Default-Modell ab Mai 2026:
  `ggml-large-v3-turbo-q8_0` (~874 MB) — Q8 ist auf modernen Backends
  gleich schnell wie Q5 bei sichtbar besserer DE-Qualität. Wählbare Slots
  inkl. `large-v3-turbo-german-q5_0` (primeline-Fine-tune, ~28 % rel.
  WER-Reduktion auf Deutsch) und `small-q5_1` für 4-GB-Geräte.
  BeamSearch (size=5) mit temperature-Fallback. **Phase-2-Streaming**:
  parallel zur Aufnahme läuft ein Greedy-Decode, das Overlay zeigt
  mitlaufenden Partial-Text. Final-Pass nach Stop-Hotkey überschreibt
  mit voller Qualität. **Phase 3a (ab Mai 2026): Vulkan-Default** —
  ein Binary deckt iGPU/AMD/Intel/NVIDIA per Vulkan ab; bei fehlendem
  GPU-Device automatischer CPU-Fallback. `gpu-cuda`/`gpu-metal`/
  `gpu-coreml` als opt-in Features, `fast-cpu` (OpenBLAS) als
  Headless-Fallback.
- **Lokales LLM:** **Embedded** ist seit Mai 2026 der Standardpfad —
  llama-cpp-2 0.1.146 mit Vulkan-Backend läuft direkt im VoiceTypeX-
  Prozess, **kein externer Daemon nötig**. Modi mit `processing = "local"`
  ohne explizites `local_engine` nutzen automatisch Embedded. Sechs
  GGUF-Slots: **Gemma 4 E4B** (Pro, 12+ GB RAM, ~5,1 GB Disk),
  **Gemma 4 E2B** (Mittel, 8-12 GB, ~3,1 GB), Gemma 3 1B (Light,
  <8 GB, ~851 MB), Gemma 3 4B (Vor-Gemma4-Pro), Llama 3.2 1B, Qwen 2.5
  1.5B. Settings-UI hat Hardware-basierte Slot-Empfehlung und
  Ein-Klick-Download. **Ollama** bleibt als Opt-in für User mit
  eigener Daemon-Installation via `local_engine = "ollama"` im
  Mode-TOML — die Konfiguration (Endpoint + Keep-Alive) sitzt in der
  Settings-Page in einem zusammenklappbaren „Ollama-Konfiguration"-
  Block.
- **Cloud-Provider (BYOK):** xAI (STT + Grok, Default `grok-4-fast-non-reasoning`),
  OpenAI Whisper + GPT, Groq Whisper, Deepgram, Anthropic Claude
- **Wayland Auto-Paste:** ashpd (RemoteDesktop-Portal) + reis (libei)
- **Secrets:** `~/.config/.../secrets.json` (chmod 0600) als Source of
  Truth, OS-Keychain best-effort Mirror (Linux-Setups mit
  gnome-keyring + kwallet sind unzuverlässig)
- **Repo & CI:** GitLab (`.gitlab-ci.yml`)

Architektur-Tiefe: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
Konventionen + Mindset: [`CLAUDE.md`](CLAUDE.md).

## Bauen (lokal)

**Voraussetzungen:**
- Rust stable über `rustup` (`rust-toolchain.toml` pinnt den Channel)
- Node.js 20+ und `pnpm` (`corepack enable && corepack prepare pnpm@latest --activate`)
- Linux: System-Pakete laut [`docs/PLATFORMS.md`](docs/PLATFORMS.md)
- Windows: WebView2 Runtime + MSVC Build Tools

```bash
pnpm install
pnpm tauri dev          # Dev-Build mit HMR fürs Frontend
pnpm tauri build        # Bundles (.deb, .rpm, AppImage, NSIS) — auf Tags v* in CI
pnpm test               # Frontend-Tests (Vitest, reine Logik wie i18n-Translate)
pnpm i18n:check         # Locale-Parity-Gate (läuft auch automatisch im prebuild)
```

Für Linux-Bundle-Build inkl. RPM zusätzlich `rpmbuild` installieren
(`sudo apt-get install rpm` auf Debian; auf Fedora bereits vorhanden).
Details, Output-Pfade und Installations-Anleitung in
[`docs/PLATFORMS.md`](docs/PLATFORMS.md) → *„Distribution-Bundles"*.

## Erste Schritte

1. **App starten:** `pnpm tauri dev`. Tray-Icon erscheint im System-Tray
   (Hauptfenster startet versteckt — siehe Wayland-Fokus-Hinweis in
   [`CLAUDE.md`](CLAUDE.md) §4.8).
2. **Hauptfenster öffnen:** Linksklick aufs Tray-Icon, oder Rechtsklick →
   *„Einstellungen öffnen"*.
3. **Whisper-Modell laden:** Tab *Einstellungen* → *„Default-Modell
   herunterladen"*. Lädt das im `whisper_default_slot` konfigurierte
   Modell (Default ab Mai 2026: `ggml-large-v3-turbo-q8_0`, ~874 MB) plus
   Silero-VAD v6.2.0 (~885 kB) mit SHA-256-Verifikation aus Hugging Face
   nach `app_data_dir/models/`. Existierende User mit Q5-Setup behalten
   Q5; nur frische Installs starten direkt mit Q8.
4. **Optional — Cloud-STT/LLM einrichten:** Tab *Einstellungen* →
   *„Cloud-API-Keys (BYOK)"* → Key bei xAI / OpenAI / etc. einfügen.
   Button *„Verbindung testen"* prüft den Key direkt.
5. **Diktat-Test (lokal):** Cursor in einem Textfeld (Browser, Editor,
   Slack-Eingabe), `Ctrl+Alt+Space` drücken, im Overlay-Menü *Exaktes
   Diktat* wählen, `Enter` zum Starten, sprich, denselben Hotkey
   nochmal zum Stoppen. Der transkribierte Text erscheint an der
   Cursor-Position.
6. **Auto-Paste auf Wayland:** Beim allerersten Diktat zeigt KDE Plasma
   einen Permission-Dialog *„VoiceTypeX möchte Tastendrücke senden"*.
   Erlauben — danach läuft Auto-Paste ohne weitere Dialoge, auch nach
   App-Restart (`restore_token` wird persistiert).

## Deinstallation

Der OS-Paket-Manager (apt/dnf/NSIS) entfernt nur das, was er installiert
hat — User-Daten unter `~/.config/de.kevin-stenzel.voicetypex/`
(Settings, Modi, API-Keys, Wayland-Token), Modelle unter `models/`
(bis zu 10 GB GGUF + Whisper-Files), OS-Keychain-Einträge und ein
eventueller Autostart-Eintrag bleiben **bewusst** liegen, damit ein
Re-Install den User-Zustand wiederfindet.

**Vor dem Uninstall:** In *Einstellungen → Gefahrenzone* kannst du
API-Keys, Wayland-Token und die App-Konfiguration einzeln oder
gemeinsam zurücksetzen.

**Vollständige Spurenbeseitigung nach dem Uninstall:**

- Linux/macOS:
  ```bash
  bash scripts/uninstall-cleanup.sh
  ```
- Windows (PowerShell, *nicht* als Admin):
  ```powershell
  powershell -ExecutionPolicy Bypass -File scripts\uninstall-cleanup.ps1
  ```

Beide Skripte sind interaktiv — jeder Schritt fragt einzeln nach.
Details und welche Spuren manuell entfernt werden müssen (KDE-Wayland-
Portal-Permission, WebView2-Cache, …) stehen in
[`docs/PLATFORMS.md`](docs/PLATFORMS.md) → *„Deinstallation"*.

## Datenschutz & Sicherheit

- Audio, Transkripte und LLM-Antworten werden standardmäßig **nicht**
  geloggt. Es existiert ein Opt-in-*Diagnose-Logging*-Toggle in den
  Einstellungen.
- **Keine** Telemetrie, **kein** Analytics.
- Cloud-API-Keys werden **niemals** im Klartext auf Disk gespeichert —
  sie liegen in `~/.config/.../secrets.json` mit chmod 0600 (plus
  best-effort Spiegel in OS-Keychain) und werden nie geloggt, nie ins
  Frontend exponiert (alle Provider-Requests gehen durch das Rust-Backend).
- Whisper-Modelle werden **nicht** im Installer mitgebündelt — Downloader
  mit Hash-Verifikation aus Hugging Face beim ersten Start.
- Beim System-Start wird **nicht** automatisch gestartet — Auto-Start
  ist explizites Opt-in.

## Lizenz

VoiceTypeX ist freie Software unter der **GNU General Public License
Version 3 oder später** (`GPL-3.0-or-later`). Volltext in
[`LICENSE`](LICENSE) (identisch in [`COPYING`](COPYING) abgelegt, gemäß
GNU-Konvention).

```
Copyright (C) 2026 Kevin Stenzel und Mitwirkende

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.
```
