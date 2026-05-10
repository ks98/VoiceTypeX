# Plattform-Notizen

## Linux

### Wayland (KDE Plasma 6, GNOME 46+)

Funktional komplett: Hotkeys via `xdg-desktop-portal.GlobalShortcuts`
(über `ashpd`), Auto-Paste via `xdg-desktop-portal.RemoteDesktop` +
`reis` (libei). Beim ersten Diktat zeigt der Compositor einen
Permission-Dialog *„VoiceTypeX möchte Tastendrücke senden"*; nach
Erlaubnis wird der `restore_token` in
`~/.config/.../wayland_session.json` (chmod 0600) persistiert — kein
Permission-Dialog bei nachfolgenden App-Starts.

**Mindestversionen:**
- `xdg-desktop-portal` ≥ 1.18
- `libei` (System-Library) ≥ 1.0
- KDE Plasma ≥ 6.1 (`xdg-desktop-portal-kde` mit MR !223 + KWin MR !5496
  gemerged) **oder** GNOME ≥ 46 (Mutter MR !2628 gemerged)

**Wayland-Fokus-Quirks** (KDE Plasma 6) sind in
[`CLAUDE.md`](../CLAUDE.md) §4.8 + §8 dokumentiert. Kurz:
- Hauptfenster startet versteckt — wird sonst beim Hotkey-Trigger
  fokussiert und klaut den Fokus von der Ziel-App.
- Overlay-Window wird vor dem libei-Inject explizit `hide()` gerufen
  (mit 80 ms Pause), damit der Fokus zurück zur Ziel-App springt.

### X11

Funktional komplett: Hotkeys via `tauri-plugin-global-shortcut`
(XGrabKey-basiert), Clipboard-Inject über
`tauri-plugin-clipboard-manager` + simulierten `Ctrl+V` per `enigo`
(XTest).

### Hyprland / Sway / wlroots

`xdg-desktop-portal-hyprland` implementiert das `RemoteDesktop`-Portal
nicht (Issue #252 offen); wlroots-Maintainer haben sich gegen den
Portal-Ansatz positioniert. Auf diesen Compositors fällt der Wayland-
Pfad auf Clipboard + Notification *„Drücke Strg+V"* zurück. Eine native
Auto-Paste-Lösung müsste über `wlr-virtual-keyboard-unstable-v1`
(z.B. via `wtype`-Sub-Prozess) gebaut werden — aktuell **nicht
implementiert** und ohne konkretes Bedürfnis auch nicht eingeplant.

### Build-Anforderungen (Debian/Ubuntu)

```bash
sudo apt-get install -y \
    build-essential pkg-config curl \
    libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev \
    libayatana-appindicator3-dev librsvg2-dev \
    libssl-dev \
    libasound2-dev          # cpal (Audio-Aufnahme) \
    libxdo-dev              # enigo (X11-Keystroke; auf reinem Wayland nicht laufzeit-relevant) \
    libclang-dev cmake      # whisper-rs (bindgen + whisper.cpp Build)
```

**Hinweis Wayland:** `reis` ist eine pure-Rust-Implementierung des
EI-Protokolls und braucht keine separate System-Library. Es kommuniziert
direkt mit dem File-Descriptor, den ashpd vom Portal liefert.
`xdg-desktop-portal-kde` bzw. `xdg-desktop-portal-gnome` muss als
Backend installiert sein (auf KDE/GNOME meist Default).

Auf Fedora/Arch sind die Paketnamen etwas anders — die Prinzip-Liste
(GTK, WebKit2GTK 4.1, Soup3, AppIndicator, RSVG, ALSA, libxdo, clang,
cmake) bleibt gleich.

### Bekannte X11-Limits

- Paste-Shortcut ist auf `Ctrl+V` festgelegt. Terminals erwarten
  oft `Ctrl+Shift+V` — Diktat in Terminal-Apps fügt nichts ein.
  Workaround: `injection_method = "keystrokes"` pro Modus (für direktes
  Tippen statt Paste — auf X11/Windows verfügbar).
- Manche WMs blockieren `XGrabKey` für bestimmte Modifier-Kombinationen
  (z.B. wenn ein WM-Shortcut die gleiche Combi schon hat). In dem Fall
  meldet `tauri-plugin-global-shortcut` einen Fehler und VoiceTypeX
  zeigt eine Notification.

## Windows

Windows 10/11 mit WebView2 (auf 11 vorinstalliert; auf 10 kommt es mit
dem Tauri-Installer).

### Build-Anforderungen

- Rust stable (`rustup` mit MSVC-Toolchain — empfohlen statt GNU)
- Node.js 20+ und pnpm (am einfachsten via `corepack enable`)
- Visual Studio Build Tools 2019+ mit *„Desktop development with C++"*
- WebView2 Runtime (auf Win 11 vorinstalliert; sonst aus
  https://developer.microsoft.com/microsoft-edge/webview2/)

### Bekannte Windows-Eigenheiten

- `cargo` zieht `whisper-rs-sys` ein, das whisper.cpp's C++-Code mit
  cmake/MSVC kompiliert. Erstmaliger Build ~5–10 min.
- `enigo` nutzt `SendInput` — funktioniert in den meisten Anwendungen,
  aber einige UWP-/WinUI-Apps haben restriktive Input-Pfade. Workaround
  mit Clipboard-Fallback (Standard).

## macOS — geplant (Phase 6)

Aktuell sind alle macOS-Implementierungen Stubs hinter
`#[cfg(target_os = "macos")]`. Geplant:
- CGEvent für Keystroke-Injection
- TCC-/Accessibility-Permission-Flow im Onboarding
- Signierter `.dmg`-Installer + Apple Notarization
- Auto-Update via tauri-plugin-updater

## CI

`.gitlab-ci.yml` baut auf jedem Push:
- Linux (Debian-Slim Container) — `cargo check + clippy + test`,
  `pnpm lint + build`
- Windows (saas-windows-medium-amd64) — `cargo check`, `pnpm build`

Auf Tags `v*` zusätzlich `pnpm tauri build` für beide Plattformen mit
Bundle-Artefakten (deb/AppImage/nsis).

> Die CI-Konfiguration wurde in Phase 1 angelegt. Bei API-Drift (Tauri
> 2.x Updates, neue System-Pakete) ist sie eventuell auf den letzten
> Stand zu bringen — beim ersten realen Push prüfen.
