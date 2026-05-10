# VoiceTypeX

Plattformübergreifendes Desktop-Werkzeug, das Diktate in Text verwandelt
und ihn an der aktuellen Cursor-Position einfügt — überall, in jeder App.
Hotkey halten, sprechen, loslassen, fertig.

## Status

Funktional komplett auf **Linux/Wayland (KDE Plasma 6 + GNOME 46+)**,
**Linux/X11** und **Windows**. Auto-Paste auf Wayland über
`xdg-desktop-portal.RemoteDesktop` + libei. Settings und Wayland-
Permission-Token persistent. Offen: macOS-Port mit Code-Signing
(roadmapped, kein ETA).

## Kernablauf

1. Globaler Hotkey **gedrückt halten** → Aufnahme startet, Tray-Icon
   pulsiert rot, Overlay zeigt *„Höre zu …"*.
2. Sprich, was du diktiert haben willst.
3. Hotkey **loslassen** → Audio wird transkribiert (lokal via
   whisper.cpp **oder** Cloud-STT), optional durch ein LLM nach dem
   aktiven **Modus** nachbearbeitet (lokal via Ollama **oder**
   Cloud-LLM), und an der Cursor-Position eingefügt.

Sechs Modi sind vorinstalliert: *Exaktes Diktat*, *Korrigierendes
Diktat*, *Förmliche E-Mail*, *Slack/Teams*, *GitHub-Issue*,
*Claude-Code-Anweisung*. Eigene Modi via TOML — siehe
[`docs/MODES.md`](docs/MODES.md).

## Tech-Stack (kompakt)

- **App-Framework:** Tauri 2 (stable)
- **Backend:** Rust 2021+ mit tokio
- **Frontend:** React 18 + TypeScript strict + Vite + TailwindCSS +
  shadcn/ui + Zustand
- **Audio:** cpal + hound (WAV) + rubato (Sinc-Resampling auf 16 kHz)
- **Lokales STT:** whisper-rs mit `ggml-large-v3-turbo-q5_0` (~547 MB,
  ~5–7 % WER auf deutschen Diktaten); Cargo-Feature-Backends
  (`fast-cpu`/OpenBLAS = Default, `gpu-vulkan`/`gpu-cuda`/`gpu-metal`/
  `gpu-coreml` opt-in)
- **Lokales LLM:** Ollama via HTTP
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
pnpm tauri build        # Bundles (.deb, AppImage, NSIS) — auf Tags v* in CI
```

## Erste Schritte

1. **App starten:** `pnpm tauri dev`. Tray-Icon erscheint im System-Tray
   (Hauptfenster startet versteckt — siehe Wayland-Fokus-Hinweis in
   [`CLAUDE.md`](CLAUDE.md) §4.8).
2. **Hauptfenster öffnen:** Linksklick aufs Tray-Icon, oder Rechtsklick →
   *„Einstellungen öffnen"*.
3. **Whisper-Modell laden:** Tab *Einstellungen* → *„Default-Modell
   herunterladen"*. Lädt `ggml-large-v3-turbo-q5_0` (~547 MB) mit
   SHA-Verifikation aus Hugging Face nach `app_data_dir/models/`.
4. **Optional — Cloud-STT/LLM einrichten:** Tab *Einstellungen* →
   *„Cloud-API-Keys (BYOK)"* → Key bei xAI / OpenAI / etc. einfügen.
   Button *„Verbindung testen"* prüft den Key direkt.
5. **Diktat-Test (lokal):** Cursor in einem Textfeld (Browser, Editor,
   Slack-Eingabe), `Ctrl+Alt+D` halten, sprich, lass los. Der
   transkribierte Text erscheint an der Cursor-Position.
6. **Auto-Paste auf Wayland:** Beim allerersten Diktat zeigt KDE Plasma
   einen Permission-Dialog *„VoiceTypeX möchte Tastendrücke senden"*.
   Erlauben — danach läuft Auto-Paste ohne weitere Dialoge, auch nach
   App-Restart (`restore_token` wird persistiert).

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
