# VoiceTypeX

Plattformübergreifendes Desktop-Werkzeug, das Diktate in Text verwandelt und ihn an der aktuellen Cursor-Position einfügt — überall, in jeder App.

**Status:** Phase 2 — xAI End-to-End plus lokales STT (Windows + Linux/X11). Wayland/macOS folgen in späteren Phasen.

## Kernablauf

1. Globaler Hotkey startet die Aufnahme.
2. Audio wird vom Standard-Mikrofon aufgenommen, mit Tray-Statusanzeige und kurzem Hinweiston.
3. Audio wird transkribiert (lokal via whisper.cpp **oder** Cloud-STT).
4. Transkript wird durch ein LLM nach dem aktiven **Modus** nachbearbeitet (lokal via Ollama **oder** Cloud-LLM). Der Modus "Exaktes Diktat" überspringt diesen Schritt.
5. Finaler Text wird an der Cursor-Position eingefügt.

## Tech-Stack (kurz)

- **Frontend:** React 18 + TypeScript + Vite + TailwindCSS + shadcn/ui + Zustand
- **Backend:** Rust (Tauri 2, tokio)
- **Audio:** cpal + hound + rubato (Resampling) + rodio (Cues)
- **Lokales STT:** whisper-rs mit `ggml-large-v3-turbo-q5_0` (~547 MB, ~5–7 % WER auf deutschen Diktaten)
- **Lokales LLM:** Ollama via HTTP
- **Cloud (BYOK):** xAI (STT + Grok), OpenAI, Anthropic, Groq, Deepgram — API-Keys liegen ausschließlich im OS-Keychain via `keyring`
- **Config:** TOML, Hot-Reload via `notify`
- **Repo-Hosting & CI:** GitLab (`.gitlab-ci.yml`)

Detaillierte Architektur-Entscheidungen: siehe [`CLAUDE.md`](CLAUDE.md) und [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) (entsteht im Verlauf von Phase 1).

## Bauen (lokal)

Voraussetzungen:
- Rust stable (über `rustup`; `rust-toolchain.toml` pinnt den Channel)
- Node.js 20+ und `pnpm` (`corepack enable && corepack prepare pnpm@latest --activate`)
- Linux: siehe [`docs/PLATFORMS.md`](docs/PLATFORMS.md) für die vollständige
  Paket-Liste (GTK3, WebKit2GTK 4.1, Soup3, AppIndicator, ALSA, libxdo,
  libclang, cmake)
- Windows: WebView2 Runtime (auf Windows 11 vorinstalliert) + MSVC Build Tools

```bash
pnpm install
pnpm tauri dev
```

## Phase-1-Test (manuell)

Das ist der Definition-of-Done-Walkthrough für Phase 1 (CLAUDE.md §6.1):

1. **App starten:** `pnpm tauri dev` — das Tray-Icon (lila V auf grauem
   Kreis = Idle) erscheint. Im Hauptfenster siehst du die Tabs
   *Einstellungen*, *Modi*, *Logs*.
2. **Default-Modi prüfen:** Tab *Modi* zeigt 6 Einträge mit ihren Hotkeys.
   `exakt` hat `CommandOrControl+Alt+D` und ist der einzige, der jetzt
   schon end-to-end funktioniert.
3. **Whisper-Modell laden:** Tab *Einstellungen*. Erstmals brauchst du das
   Default-Modell unter
   `app_data_dir/models/ggml-large-v3-turbo-q5_0.bin`. Phase 1 hat noch
   keinen Download-Button — aktuell musst du es selbst von
   <https://huggingface.co/ggerganov/whisper.cpp> herunterladen und im
   Settings-Tab den Pfad setzen (oder direkt nach `app_data_dir/models/`
   legen).
4. **Diktat-Test:** Cursor in einem Textfeld (Browser, Notepad, beliebige
   App), `Ctrl+Alt+D` drücken. Tray-Icon wird rot, kurzer Beep. Sprich.
   `Ctrl+Alt+D` erneut → Beep, Icon wird gelb (Transcribing) → grün
   (Injecting) → grau (Idle). Der transkribierte Text steht an der
   Cursor-Position.
5. **Andere Hotkeys:** `Ctrl+Alt+E` (E-Mail-Modus) → Notification "Modus
   wird in einer späteren Phase implementiert." Korrekt.
6. **Modi-Hot-Reload:** Editiere `app_config_dir/modes/exaktes_diktat.toml`
   im Editor (z.B. ändere `name`). Speichern. Tab *Modi* aktualisiert
   sich nach ~1 Sekunde.

## Phase-2-Test (Cloud / xAI End-to-End)

Nachdem Phase 1 sauber läuft:

1. **xAI-Key beziehen:** Account auf <https://console.x.ai>, API-Key
   generieren. Derselbe Key wird für STT und LLM (Grok) verwendet.
2. **Key hinterlegen:** Tab *Einstellungen* → "Cloud-API-Keys (BYOK)"
   → "Setzen" neben *xAI*. Key einfügen, speichern.
3. **Verbindung testen:** Button "Verbindung testen" neben *xAI*.
   Erfolgsmeldung "✓ Verbindung erfolgreich" sollte erscheinen.
4. **Cloud-Diktat testen:** Cursor in einem E-Mail-Entwurf,
   `Ctrl+Alt+E` drücken → Beep → diktiere → `Ctrl+Alt+E` →
   Tray-Icon zeigt sequentiell Recording → Transcribing → Postprocessing
   → Injecting → Idle. Der formell ausformulierte E-Mail-Text
   landet an der Cursor-Position.
5. **Andere Cloud-Modi:** `Ctrl+Alt+S` (Slack), `Ctrl+Alt+G` (GitHub Issue),
   `Ctrl+Alt+C` (Claude-Code-Anweisung) — alle nutzen denselben xAI-Key.
6. **Lokales LLM-Diktat (`Ctrl+Alt+K`):** braucht zusätzlich einen
   laufenden Ollama-Server (`ollama serve`) und das gewählte Modell
   (`ollama pull qwen2.5:7b`). Konfiguration in den Einstellungen unter
   "Ollama-Endpunkt".

## Datenschutz

## Datenschutz

- Audio, Transkripte und LLM-Antworten werden standardmäßig **nicht** geloggt. Es existiert ein opt-in-"Diagnose-Logging"-Toggle in den Einstellungen.
- Es gibt **keine** Telemetrie und kein Analytics.
- Cloud-API-Keys werden **niemals** im Klartext auf Disk gespeichert — sie liegen im OS-Keychain.
- Whisper-Modelle werden beim ersten Start **nicht** im Installer mitgebündelt, sondern via Downloader mit Hash-Verifikation aus Hugging Face geholt.
- Beim System-Start wird **nicht** automatisch gestartet — Auto-Start ist explizites Opt-in.

## Lizenz

VoiceTypeX ist freie Software unter der **GNU General Public License Version 3 oder später** (`GPL-3.0-or-later`).

Den vollständigen Lizenztext findest du in [`LICENSE`](LICENSE) (identisch in [`COPYING`](COPYING) abgelegt, gemäß GNU-Konvention).

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
