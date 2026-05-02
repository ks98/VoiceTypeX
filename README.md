# VoiceTypeX

Plattformübergreifendes Desktop-Werkzeug, das Diktate in Text verwandelt und ihn an der aktuellen Cursor-Position einfügt — überall, in jeder App.

**Status:** Phase 1 — lokales STT auf Windows + Linux/X11. Cloud-Provider und Wayland/macOS folgen in späteren Phasen.

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
- Linux: System-Pakete für Tauri 2 (`libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`)
- Windows: WebView2 Runtime (auf Windows 11 vorinstalliert)

```bash
pnpm install
pnpm tauri dev
```

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
