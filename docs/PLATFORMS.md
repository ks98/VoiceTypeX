# Plattform-Notizen

Stand Phase 1: Linux/X11 und Windows 10/11 sind die Zielplattformen. Wayland
und macOS folgen in späteren Phasen.

## Linux

### Zielplattform Phase 1: X11

Funktioniert auf X11-Sessions (Xorg, oder Xwayland-Compatibility).
Hotkeys laufen über `tauri-plugin-global-shortcut` (XGrabKey-basiert),
Clipboard-Inject über `tauri-plugin-clipboard-manager` + simulierten
`Ctrl+V` per `enigo`.

#### Build-Anforderungen (Debian/Ubuntu)

```bash
sudo apt-get install -y \
    build-essential pkg-config curl \
    libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev \
    libayatana-appindicator3-dev librsvg2-dev \
    libssl-dev \
    libasound2-dev          # cpal (Audio-Aufnahme)
    libxdo-dev              # enigo (Keystroke)
    libclang-dev cmake      # whisper-rs (bindgen + whisper.cpp Build)
```

Auf Fedora/Arch sind die Paketnamen etwas anders – die Prinzip-Liste
(GTK, WebKit2GTK 4.1, Soup3, AppIndicator, RSVG, ALSA, libxdo, clang,
cmake) bleibt gleich.

### Wayland: Phase 5

Aktuell gibt der Wayland-Pfad einen klaren Fehler zurück:
"Wayland-Support kommt in Phase 5". Der Detection läuft per
`std::env::var("WAYLAND_DISPLAY")` (siehe `injection/mod.rs`,
`hotkey/mod.rs`).

Geplant:
- `xdg-desktop-portal.GlobalShortcuts` für Hotkey-Registrierung
- `libei` über das `RemoteDesktop`-Portal für Keystroke-Injection
- Compositor-Kompatibilität: KDE Plasma 5.27+, GNOME 45+, Hyprland, Sway

### Bekannte X11-Limits

- Paste-Shortcut ist auf `Ctrl+V` festgelegt. Terminals erwarten
  `Ctrl+Shift+V` — Diktat in Terminal-Apps wird Phase 1 keinen Text
  einfügen. Workaround in Phase 4: app-spezifischer Override pro Modus.
- Manche WMs blockieren `XGrabKey` für bestimmte Modifier-Kombinationen
  (z.B. wenn ein WM-Shortcut die gleiche Combi schon hat). In dem Fall
  meldet `tauri-plugin-global-shortcut` einen Fehler und VoiceTypeX zeigt
  eine Notification.

## Windows

Windows 10/11 mit WebView2 (auf 11 vorinstalliert; auf 10 kommt es mit
dem Tauri-Installer).

### Build-Anforderungen

- Rust stable (`rustup` mit MSVC-Toolchain — empfohlen statt GNU)
- Node.js 20+ und pnpm (am einfachsten via `corepack enable`)
- Visual Studio Build Tools 2019+ mit "Desktop development with C++"
- WebView2 Runtime (auf Win 11 vorinstalliert; sonst aus
  https://developer.microsoft.com/microsoft-edge/webview2/)

### Bekannte Windows-Eigenheiten

- `cargo` zieht `whisper-rs-sys` ein, das whisper.cpp's C++-Code mit
  cmake/MSVC kompiliert. Erstmaliger Build ~5–10 min.
- `enigo` nutzt `SendInput` — funktioniert in den meisten Anwendungen, aber
  einige UWP-/WinUI-Apps haben restriktive Input-Pfade. Workaround mit
  Clipboard-Fallback (Standard).

## macOS: Phase 6

Aktuell sind alle macOS-Implementierungen Stubs hinter
`#[cfg(target_os = "macos")]`. Geplant:
- CGEvent für Keystroke-Injection
- TCC-/Accessibility-Permission-Flow im Onboarding
- Signierter `.dmg`-Installer + Notarisierung
- Auto-Update via Tauri-Plugin

## CI-Matrix

`.gitlab-ci.yml` baut auf jedem Push:

- Linux (Debian-Slim Container) — `cargo check + clippy + test`,
  `pnpm lint + build`
- Windows (saas-windows-medium-amd64) — `cargo check`, `pnpm build`

Auf Tags `v*` zusätzlich `pnpm tauri build` für beide Plattformen mit
Bundle-Artefakten (deb/AppImage/nsis).
