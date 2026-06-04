# VoiceTypeX

A cross-platform desktop tool that turns dictation into text and inserts
it at the current cursor position — anywhere, in any app. A global hotkey
opens a mode menu, arrows + Enter select, dictation runs — and the same
hotkey stops it.

## Status

**Beta** — feature-complete on **Linux/Wayland (KDE Plasma 6 +
GNOME 46+)**, **Linux/X11**, and **Windows**. Auto-paste on Wayland via
`xdg-desktop-portal.RemoteDesktop` + libei. Settings and the Wayland
permission token persist. Release bundles for Linux
(`.deb` / `.rpm` / AppImage) with a signed auto-updater are available.
The **Windows release** includes speech recognition (whisper.cpp + Vulkan)
and cloud LLM post-processing; the **embedded local LLM (llama-cpp-2) is
Linux/macOS-only** — on Windows its ggml symbols collided with those of
whisper.cpp during MSVC linking (LNK2005,
[Issue #1](https://github.com/ks98/VoiceTypeX/issues/1)), so the local LLM
runs there via a self-installed **Ollama** daemon or a cloud provider.
macOS is out of scope.

Beta-specific notes: see the section
[*"Beta Status & Updates"*](#beta-status--updates) further down.

## Core Flow

1. Press the global **menu hotkey** (default `Ctrl+Alt+Space`) → the
   overlay shows the mode list, with the cursor on the most recently
   used selection.
2. Use `↑`/`↓` to pick a different mode (or hit `Enter` directly to
   confirm the last-used selection). `Esc` closes the menu without
   taking any action.
3. After `Enter`, recording starts, the tray icon pulses red, and the
   overlay shows *"Listening …"* plus a status line with the active
   engine (local vs cloud, STT/LLM model). Speak.
4. **Press the same hotkey again** → the audio is transcribed (locally
   via whisper.cpp **or** cloud STT), optionally post-processed by an LLM
   according to the selected **mode** (locally via Ollama **or** a cloud
   LLM), and inserted at the cursor position.

Nine modes ship preinstalled. Six **dictation** modes (new text):
*Exact Dictation*, *Corrective Dictation*, *Formal Email*,
*Slack/Teams Message*, *GitHub Issue*, *Instruction to Coding Agent*.
Plus three **edit** modes that transform *selected* text:
*Improve* (rewrites/replaces the selection), *Write Reply*
(composes a reply below it, leaving the original intact), and *Free Edit*
(via voice command, with the LLM deciding where to place the result).
Workflow: select → hotkey → choose an edit mode → speak the instruction.
The defaults ship **locale-aware**: with an English UI locale you get
English modes with English `system_prompt`s, and with a French/Spanish/
Italian locale the respective cultural equivalents (forms of address,
dictation commands). Custom modes via TOML — see
[`docs/MODES.md`](docs/MODES.md).

**Migrating from older versions:** modes used to each have their own
hotkey (`Ctrl+Alt+D`, `Ctrl+Alt+E`, …). The switch to the single menu
hotkey is transparent — existing `hotkey` fields in `modes/*.toml` are
still accepted but ignored. You change the menu hotkey itself under
*Settings* → *Global Menu Hotkey*.

## Tech Stack (at a glance)

- **App framework:** Tauri 2 (stable)
- **Backend:** Rust 2021+ with tokio
- **Frontend:** React 18 + TypeScript strict + Vite + TailwindCSS +
  shadcn/ui + Zustand
- **Internationalization:** a custom `useT()` hook (~70 LOC, no i18next)
  with `Intl.PluralRules`. Fully shipped in `de`, `en`, `fr`, `es`, `it`:
  all UI strings (~400), the tray menu, and per-locale default modes with
  culturally adapted `system_prompt`s. OS locale detection in the backend
  (`tauri_plugin_os::locale()`), live switching via Settings without an
  app restart. The build gate `pnpm i18n:check` validates locale parity
  and used-key existence.
- **Audio:** cpal + hound (WAV) + rubato (sinc resampling to 16 kHz)
- **Local STT:** whisper-rs 0.16 with Silero VAD v6 (prevents
  silence hallucinations). Default model since May 2026:
  `ggml-large-v3-turbo-q8_0` (~874 MB) — on modern backends Q8 is just as
  fast as Q5 while delivering visibly better DE quality. Selectable slots
  include the German fine-tune (`large-v3-turbo-german-q5_0` and the
  Vulkan-safe `-q8_0`, primeline/Apache-2.0 — best German accuracy) and
  `small-q5_1` for 4 GB devices. The picker shows each model with
  Speed/Accuracy bars + a hardware-aware recommendation. BeamSearch
  (default size 2, tuned for low dictation latency) with a temperature
  fallback. **Phase-2 streaming**:
  a greedy decode runs in parallel with recording, and the overlay shows
  the partial text as it comes in. The final pass after the stop hotkey
  overwrites it at full quality. **Phase 3a (since May 2026): Vulkan
  default** — a single binary covers iGPU/AMD/Intel/NVIDIA via Vulkan,
  with an automatic CPU fallback when no GPU device is present.
  `gpu-cuda`/`gpu-metal`/`gpu-coreml` as opt-in features, and `fast-cpu`
  (OpenBLAS) as the headless fallback.
- **Local LLM:** **Embedded** has been the default path since May 2026
  (**Linux/macOS-only**) — llama-cpp-2 0.1.146 with the Vulkan backend
  runs directly in the VoiceTypeX process, **no external daemon needed**.
  Modes with `processing = "local"` and no explicit `local_engine`
  automatically use Embedded. **On Windows** Embedded is not compiled
  (Issue #1, ggml symbol collision during MSVC linking); there
  `local_engine` defaults to `"ollama"` — local LLM via a self-installed
  Ollama or via the cloud. Six GGUF slots: **Gemma 4 E4B**
  (Pro, 12+ GB RAM, ~5.1 GB disk),
  **Gemma 4 E2B** (Medium, 8-12 GB, ~3.1 GB), Gemma 3 1B (Light,
  <8 GB, ~851 MB), Gemma 3 4B (pre-Gemma4 Pro), Llama 3.2 1B, Qwen 2.5
  1.5B. The Settings UI provides a hardware-based slot recommendation and
  one-click download. **Ollama** remains an opt-in for users with their
  own daemon installation via `local_engine = "ollama"` in the
  mode TOML — its configuration (endpoint + keep-alive) lives in the
  Settings page in a collapsible "Ollama Configuration" block.
- **Cloud providers (BYOK):** xAI (STT + Grok, default `grok-4-fast-non-reasoning`),
  OpenAI Whisper + GPT, Groq Whisper, Deepgram, Anthropic Claude
- **Wayland auto-paste:** ashpd (RemoteDesktop portal) + reis (libei)
- **Secrets:** `~/.config/.../secrets.json` (chmod 0600), **encrypted
  at rest**: Windows uses DPAPI (`CryptProtectData`), and Linux uses
  AES-256-GCM with a 32-byte random KEK from the OS keyring
  (libsecret / kwallet). If no keyring is available, storage falls back
  to plaintext and shows a red banner warning in the API Keys tab. On
  macOS, the beta scope stays on plaintext (Security.framework
  integration after 1.0).
- **Repo & CI:** GitHub (GitHub Actions)

Architecture in depth: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
Conventions + mindset: [`CLAUDE.md`](CLAUDE.md).

## Building (locally)

**Prerequisites:**
- Rust stable via `rustup` (`rust-toolchain.toml` pins the channel)
- Node.js 20+ and `pnpm` (`corepack enable && corepack prepare pnpm@latest --activate`)
- Linux: system packages per [`docs/PLATFORMS.md`](docs/PLATFORMS.md)
- Windows: WebView2 Runtime + MSVC Build Tools

```bash
pnpm install
pnpm tauri dev          # Dev build with HMR for the frontend
pnpm tauri build        # Bundles (.deb, .rpm, AppImage, NSIS) — on v* tags in CI
pnpm test               # Frontend tests (Vitest, pure logic like i18n translation)
pnpm i18n:check         # Locale parity gate (also runs automatically in prebuild)
```

For a Linux bundle build including RPM, additionally install `rpmbuild`
(`sudo apt-get install rpm` on Debian; already present on Fedora).
Details, output paths, and installation instructions in
[`docs/PLATFORMS.md`](docs/PLATFORMS.md) → *"Distribution Bundles"*.

**Cutting a release** (bump the version, tag, CI pipeline, auto-update,
signing key): see [`docs/RELEASING.md`](docs/RELEASING.md).

## Getting Started

1. **Start the app:** `pnpm tauri dev`. The tray icon appears in the
   system tray (the main window starts hidden — see the *"Wayland focus
   quirks"* note in [`docs/PLATFORMS.md`](docs/PLATFORMS.md)). On first
   launch an onboarding wizard opens; its header has a language picker —
   the UI language is auto-detected from your OS locale, but on Windows
   that can be the display language rather than your region, so switch it
   there if it guessed wrong (also under *Settings → Language* later).
2. **Open the main window:** left-click the tray icon, or right-click →
   *"Open Settings"*.
3. **Load a Whisper model:** *Settings* tab → *"Download Default
   Model"*. This downloads the model configured in `whisper_default_slot`
   (default since May 2026: `ggml-large-v3-turbo-q8_0`, ~874 MB) plus
   Silero VAD v6.2.0 (~885 kB) with SHA-256 verification from Hugging Face
   to `app_config_dir/models/`. Existing users on a Q5 setup keep Q5;
   only fresh installs start directly on Q8.
4. **Optional — set up cloud STT/LLM:** *Settings* tab →
   *"Cloud API Keys (BYOK)"* → paste a key for xAI / OpenAI / etc.
   The *"Test Connection"* button verifies the key right away.
5. **Dictation test (local):** put the cursor in a text field (browser,
   editor, Slack input), press `Ctrl+Alt+Space`, choose *Exact
   Dictation* in the overlay menu, hit `Enter` to start, speak, then
   press the same hotkey again to stop. The transcribed text appears at
   the cursor position.
6. **Auto-paste on Wayland:** on the very first dictation, KDE Plasma
   shows a permission dialog *"VoiceTypeX wants to send keystrokes"*.
   Allow it — after that, auto-paste runs without further dialogs, even
   after an app restart (the `restore_token` is persisted).
7. **Dictating into terminals:** terminals (Konsole, GNOME Terminal, …)
   paste with `Ctrl+Shift+V`, not `Ctrl+V`. The per-mode `paste_shortcut`
   field (`auto` | `ctrl_v` | `ctrl_shift_v`) handles this — on KDE Plasma 6
   `auto` auto-detects the focused terminal; elsewhere set `ctrl_shift_v`
   for a terminal mode. See [`docs/MODES.md`](docs/MODES.md).

## Uninstalling

The OS package manager (apt/dnf/NSIS) only removes what it installed —
user data under `~/.config/de.kevin-stenzel.voicetypex/`
(settings, modes, API keys, Wayland token), models under `models/`
(up to 10 GB of GGUF + Whisper files), OS keychain entries, and any
autostart entry are **deliberately** left in place, so that a reinstall
finds the user's state again.

**Before uninstalling:** under *Settings → Danger Zone* you can reset
API keys, the Wayland token, and the app configuration individually or
all together.

**Complete trace removal after uninstalling:**

- Linux/macOS:
  ```bash
  bash scripts/uninstall-cleanup.sh
  ```
- Windows (PowerShell, *not* as admin):
  ```powershell
  powershell -ExecutionPolicy Bypass -File scripts\uninstall-cleanup.ps1
  ```

Both scripts are interactive — every step asks individually. Details and
which traces have to be removed manually (KDE Wayland portal permission,
WebView2 cache, …) are in
[`docs/PLATFORMS.md`](docs/PLATFORMS.md) → *"Uninstalling"*.

## Privacy & Security

- Audio, transcripts, and LLM responses are **not** logged. The
  `LogRingBuffer` (visible in the *Logs* tab) filters out provider
  response bodies and transcript snippets — only status codes, provider
  names, and duration metrics make it into the logs.
- **No** telemetry, **no** analytics.
- **Cloud API keys are stored encrypted at rest**: on Windows via DPAPI,
  on Linux with AES-256-GCM (KEK in the OS keyring). On Linux systems
  without a keyring, storage falls back to plaintext with chmod 0600; in
  that case the API Keys tab shows a red warning. Keys are never logged
  and never exposed to the frontend — all provider requests go through
  the Rust backend. See [`SECURITY.md`](SECURITY.md) for details.
- A **Content Security Policy** is active on all webviews: only `'self'`
  + explicit provider hosts (api.anthropic.com, api.openai.com,
  api.x.ai, api.groq.com, api.deepgram.com, huggingface.co). No inline
  scripting, no eval.
- Whisper models are **not** bundled in the installer — a downloader
  with SHA-256 hash verification fetches them from Hugging Face on first
  launch.
- The app does **not** start automatically at system startup —
  auto-start is an explicit opt-in.

## Beta Status & Updates

- **Auto-updater (Windows + AppImage).** On request, VoiceTypeX checks
  for new versions (*Settings → Diagnostics → Updates*) and installs
  them with one click. The update artifacts are signed with a
  minisign/Ed25519 key; the updater refuses unsigned or tampered
  packages. Self-update applies to the **Windows NSIS installer** and
  the **Linux AppImage**; you update **`.deb`/`.rpm`** via your
  **package manager** or by re-downloading. The download only starts on
  click (the bundle is large).
- **The Windows installer is not (yet) Authenticode-signed.** On first
  launch, SmartScreen shows "Unknown publisher" → *More info → Run
  anyway*. This is independent of the minisign updater signature.
  Distribution is exclusively via the official GitHub releases — please
  do not obtain it from third-party sources or mirrors.
- **Linux prerequisite for encryption at rest:** libsecret
  (gnome-keyring) or kwallet must be installed. Headless/server setups
  run in the plaintext fallback with a clear UI warning.
- **Bug reports:** [GitHub Issues](https://github.com/ks98/VoiceTypeX/issues)
  including `~/.config/de.kevin-stenzel.voicetypex/voicetypex.log`
  (if enabled) and the app version from *Settings → Diagnostics*.

## License

VoiceTypeX is free software under the **GNU General Public License
Version 3 or later** (`GPL-3.0-or-later`). Full text in
[`LICENSE`](LICENSE) (also stored identically in [`COPYING`](COPYING),
per GNU convention).

```
Copyright (C) 2026 Kevin Stenzel and contributors

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU General Public License for more details.
```
