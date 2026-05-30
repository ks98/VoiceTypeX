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
    libdbus-1-dev                # keyring -> dbus-secret-service (KEK fuer secrets.json at-rest) \
    libasound2-dev               # cpal (Audio-Aufnahme) \
    libxdo-dev                   # enigo (X11-Keystroke; auf Wayland nicht runtime-relevant) \
    libclang-dev cmake           # whisper-rs (bindgen + whisper.cpp-Build) \
    libvulkan-dev                # gpu-vulkan-Feature (Phase-3a-Default), Headers + Loader \
    glslc                        # GLSL->SPIR-V Shader-Compiler, von whisper.cpp's Vulkan-Backend zur Build-Zeit gebraucht \
    mesa-vulkan-drivers          # llvmpipe-Software-Vulkan-Fallback (Systeme ohne Hardware-Vulkan)
```

**Vulkan-Default (Phase 3a, Mai 2026):**
Der Default-Build nutzt das `gpu-vulkan`-Feature von `whisper-rs`.
Vulkan deckt iGPU + AMD + Intel Arc + NVIDIA mit *einem* Binary ab
(~95 % der Consumer-Hardware). Wenn zur Laufzeit kein Vulkan-Device
verfuegbar ist, faellt whisper.cpp transparent auf CPU zurueck —
kein App-Code-Pfad noetig.

`glslc` ist Build-Time-only: whisper.cpp kompiliert seine GLSL-Shader
zu SPIR-V beim Cargo-Build. Zur Laufzeit ist nur `libvulkan1` +
GPU-Treiber gefragt.

Wenn du kein Vulkan willst (Server/Container/Headless), explizit:
```bash
cargo build --no-default-features --features custom-protocol,fast-cpu
```
Das `fast-cpu`-Feature linkt OpenBLAS statt Vulkan. Build-Voraussetzung
dafuer: `libopenblas-dev` und `BLAS_INCLUDE_DIRS` gesetzt (siehe unten).

**Phase 3b — llama-cpp-sys-2 0.1.146 Build-Quirk (automatisiert):**
llama-cpp-sys-2 0.1.146's build.rs hat einen TOC/TOU-Bug — `Path::
exists()` folgt Symlinks und liefert false fuer dangling Links,
`std::fs::hard_link()` schlaegt aber fehl, weil der Symlink-Eintrag
noch da ist. Resultat ohne Workaround: `Os { code: 17, kind:
AlreadyExists }`-Panic beim Rebuild.

**Automatisiert via npm-Hook:** `scripts/clean-dangling-libs.mjs`
laeuft als `predev` und `prebuild` (siehe `package.json`) vor jedem
`pnpm tauri dev` und `pnpm tauri build`. Pure-Node, cross-platform,
no-op auf Windows. Falls du Cargo direkt aufrufst (z.B. `cargo build`
fuer Tests), entweder Tauri-Workflow nutzen oder manuell:
```bash
node scripts/clean-dangling-libs.mjs
```

**Phase 3b — `dynamic-link` Runtime-Erwartungen + Bundle-Pipeline:**
llama-cpp-2 wird mit `dynamic-link`-Feature gelinkt; das produziert
`libllama.so`, `libggml.so`, `libggml-cpu.so`, `libggml-vulkan.so`
und `libggml-base.so` als separate Shared-Libs.

- **Dev-Build (`pnpm tauri dev`):** Cargo legt die Libs in
  `target/debug/` ab und setzt rpath automatisch dorthin. Binary
  findet sie zur Laufzeit ohne Zusatzkonfiguration.
- **Distribution-Build (`pnpm tauri build`):**
  1. Cargo baut Binary + Libs in `target/release/`.
  2. `beforeBundleCommand` triggert `scripts/bundle-libs.mjs`, der
     die Libs nach `src-tauri/resources/lib/` kopiert.
  3. `tauri.conf.json`-Eintrag `bundle.resources: ["resources/lib/*"]`
     packt sie ins finale Bundle.
  4. `src-tauri/build.rs` setzt mehrere rpath-Fallback-Entries
     (`$ORIGIN`, `$ORIGIN/lib`, `$ORIGIN/../lib/voicetypex`,
     `$ORIGIN/../lib`) — egal wo der Tauri-Bundler die Libs hinlegt,
     der Linker findet sie.

`src-tauri/resources/lib/` ist gitignored ausser `.gitkeep` — Inhalt
wird bei jedem Bundle-Build neu erzeugt, gehoert nicht ins Repo.

**Optionaler CUDA-Builder-Pfad (Task #27):**
Wer auf einer Maschine mit CUDA-Toolkit baut, kann zusaetzlich
das `embedded-cuda-dynamic`-Feature aktivieren:
```bash
sudo apt install -y nvidia-cuda-toolkit  # oder vendor-Download
cargo build --release --features embedded-cuda-dynamic
```
Damit baut llama-cpp-2 die GGML-Backends als separate Shared-Libs
(`ggml-cpu.so`, `ggml-vulkan.so`, `ggml-cuda.so`); zur Laufzeit waehlt
llama.cpp das schnellste verfuegbare. Auf User-Maschinen ohne CUDA-
Treiber faellt die App transparent auf Vulkan zurueck — keine
Code-Anpassung noetig.

**Verifikation auf erstem Bundle-Build noetig** — Tauri-Bundler-
Layout-Details koennen je nach Format (.deb/.rpm/AppImage)
abweichen. Wenn der Test-Install meldet `error while loading shared
libraries: libllama.so: cannot open shared object file`, dann ist
keiner der rpath-Pfade getroffen worden — Bundle-Inspect mit
`dpkg-deb -c xyz.deb` zeigt das tatsaechliche Layout, danach
build.rs-rpath-Entries entsprechend ergaenzen.

**BLAS_INCLUDE_DIRS (nur fuer `fast-cpu`-Feature):**
Wenn `fast-cpu` aktiv ist, braucht `whisper-rs-sys` 0.15+
`BLAS_INCLUDE_DIRS` explizit. Auf Debian/Ubuntu liegt der Pfad bei
`/usr/include/x86_64-linux-gnu/openblas-pthread` —
`src-tauri/.cargo/config.toml` setzt diesen Default mit `force = false`.
Auf anderen Distros manuell setzen:

```bash
# Fedora (libopenblas-devel):
export BLAS_INCLUDE_DIRS=/usr/include/openblas
# Arch (openblas):
export BLAS_INCLUDE_DIRS=/usr/include
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

- Paste-Shortcut ist im Clipboard-Pfad auf `Ctrl+V` festgelegt. Terminals
  erwarten oft `Ctrl+Shift+V` — dort fügt der Clipboard-Pfad nichts ein.
  Workaround: im Modus `injection_method = "keystrokes"` setzen (siehe
  unten — funktioniert auf X11 und Windows; auf Wayland ignoriert).
- Manche WMs blockieren `XGrabKey` für bestimmte Modifier-Kombinationen
  (z.B. wenn ein WM-Shortcut die gleiche Combi schon hat). In dem Fall
  meldet `tauri-plugin-global-shortcut` einen Fehler und VoiceTypeX
  zeigt eine Notification.

### Keystrokes-Modus (X11 + Windows)

Modi mit `injection_method = "keystrokes"` umgehen das Clipboard
komplett. Der Text wird Zeichen für Zeichen via `enigo.text(...)` getippt
(Windows: `SendInput`, X11: `XTest`). Vorteile: funktioniert in Terminals
mit `Ctrl+Shift+V`-Paste, in IME-empfindlichen Apps und in Eingaben mit
Clipboard-Blockern. Nachteile: langsamer als Paste, Layout-abhängig —
Unicode-Zeichen außerhalb des aktiven Tastatur-Layouts können scheitern.

**Auf Wayland** wird `keystrokes` aktuell auf den libei-Clipboard-Pfad
zurückgeführt (mit Hinweis-Log). Echte Keystroke-Injection via libei
würde Char→Keysym-Mapping per `xkbcommon` brauchen — bisher nicht
implementiert.

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
- **Vulkan-SDK fuer Build-Zeit** (Phase-3a-Default): Lunarg-Vulkan-SDK
  installieren (https://www.lunarg.com/vulkan-sdk/), Environment-
  Variable `VULKAN_SDK` muss gesetzt sein. Runtime: aktuelle GPU-
  Treiber bringen `vulkan-1.dll` automatisch mit (NVIDIA/AMD/Intel).
- **Wer ohne Vulkan bauen will** (z.B. Headless oder Lizenz-strikt):
  `cargo build --no-default-features --features custom-protocol,fast-cpu`.
  Dann gilt `BLAS_INCLUDE_DIRS` (OpenBLAS-Windows-Distribution noetig,
  z.B. `set BLAS_INCLUDE_DIRS=C:\OpenBLAS\include`).
- `enigo` nutzt `SendInput` — funktioniert in den meisten Anwendungen,
  aber einige UWP-/WinUI-Apps haben restriktive Input-Pfade. Workaround
  mit Clipboard-Fallback (Standard).

### Windows — Embedded-LLM DLL-Bundle (offener Verifikations-Punkt)

`llama-cpp-2` wird mit `dynamic-link` gelinkt; auf Windows entstehen
dadurch `llama.dll`, `ggml.dll`, `ggml-cpu.dll`, `ggml-vulkan.dll` und
`ggml-base.dll` neben dem Binary in `target/release/`.
`scripts/bundle-libs.mjs` kopiert sie cross-platform nach
`src-tauri/resources/lib/`, der Tauri-NSIS-Bundler legt sie damit auf der
Ziel-Maschine unter `$INSTDIR\resources\lib\` ab.

**Problem:** Der Windows-DLL-Loader sucht zur Laufzeit in
`$INSTDIR\` (App-Dir), nicht in `$INSTDIR\resources\lib\`. Beim ersten
LLM-Aufruf droht *„llama.dll not found"*.

**Verifikation auf erstem Windows-Bundle-Build:**

1. NSIS-Installer ausführen, App starten.
2. Im Modes-Settings einen Embedded-LLM-Modus auswählen, einmal diktieren.
3. Wenn die App weiterläuft → DLLs werden gefunden, alles okay.
4. Wenn *„The code execution cannot proceed because llama.dll was not
   found"* erscheint:
   - **Workaround per Hand:** DLLs aus `$INSTDIR\resources\lib\*.dll`
     neben die `voicetypex.exe` (`$INSTDIR\`) kopieren.
   - **Permanente Lösung:** NSIS-Hook in `tauri.conf.json` ergänzen, der
     die DLLs beim Install kopiert. Skizze:
     ```nsis
     ; In bundle.windows.nsis.installerHooks → NSIS_HOOK_POSTINSTALL
     CopyFiles "$INSTDIR\resources\lib\*.dll" "$INSTDIR"
     ```
     Tauri 2's NSIS-Template-Override-Feature dokumentieren wir hier
     nach erstem realen Test (Doku-Stand zum Zeitpunkt der Notiz: Hook-
     API in Tauri 2.x in Bewegung).

## Bearbeiten-Modi: Selektion lesen & zurückschreiben

Modi mit `input = "selection"` lesen den markierten Text der fokussierten
Fremd-App und schreiben das Ergebnis zurück. Beides ist plattformabhängig
und gehört zu den manuell zu verifizierenden Pfaden.

**Lesen** (`TextInjector::read_selection`):

| Plattform | Lese-Mechanismus | Status |
|---|---|---|
| Linux X11 | **PRIMARY-Selection** direkt (arboard, nativ) | fokus-unabhängig, ohne Ctrl+C |
| Linux Wayland | **PRIMARY-Selection** via wlr/ext-data-control (arboard, Feature `wayland-data-control`) | verifiziert lesbar auf KWin (auch unfokussiert) |
| Windows | `enigo` Ctrl+C → Clipboard sichern/lesen/wiederherstellen | **manuell zu verifizieren** |

Auf Linux liegt der markierte Text automatisch in der PRIMARY-Selection
(dem „Middle-Click-Paste"-Puffer) — er wird direkt gelesen, ohne
Tastatur-Simulation, ohne Fokus, ohne die normale Zwischenablage
(CLIPBOARD) anzufassen. Windows kennt keine PRIMARY-Selection, daher dort
weiterhin der simulierte Ctrl+C-Weg.

**Zurückschreiben** richtet sich nach `output`: `replace`/`insert` fügen
direkt ein (Paste über eine aktive Selektion überschreibt sie);
`append`/`prepend` kollabieren die Selektion vorher per Pfeiltaste
(rechts/links) und fügen dann ein.

### Manuelle Verifikation (pro Plattform durchführen)

1. Text in einer Fremd-App markieren (Editor **und** Browser-Textfeld —
   die sind am unterschiedlichsten).
2. Hotkey → Bearbeiten-Modus „Verbessern" wählen → Anweisung sprechen →
   Stop. Erwartung: markierter Text wird durch das Ergebnis ersetzt.
3. „Antwort schreiben" auf einen markierten Absatz: Original bleibt
   stehen, Antwort erscheint darunter.
4. „Frei bearbeiten": je nach Anweisung ersetzen vs. anhängen
   (`@@`-Steuerzeile).
5. Nichts markieren, Hotkey, Bearbeiten-Modus: PRIMARY ist leer → leere
   Selektion (Log: `Eager selection capture captured=false`).

### Bekannte Grenzen & Risiken

- **PRIMARY-Semantik (Linux):** PRIMARY enthält den *zuletzt markierten*
  Text — egal in welchem Fenster. Hat der Nutzer nichts Frisches
  markiert, wird eine veraltete/fremde Selektion gelesen. In der Praxis
  unkritisch, weil der Workflow „markieren → sofort Hotkey" ist.
- **Apps ohne PRIMARY (Linux):** Manche Chromium-/Electron-Builds und
  einige Terminals füllen PRIMARY nicht zuverlässig → `read_selection`
  liefert `None`, der Edit-Modus arbeitet dann auf leerer Selektion.
- **`append`/`prepend` brauchen die überlebende Selektion (Output):** Das
  Zurückschreiben kollabiert die Selektion per Pfeiltaste. Apps, die die
  Selektion beim Fokuswechsel (Menü → Ziel-App) verwerfen, lassen das
  Kollabieren ins Leere laufen. Das Lesen ist davon nicht betroffen
  (PRIMARY persistiert fokus-unabhängig).
- **Windows-`read_selection`-Heuristik:** „nichts markiert" wird über
  „Clipboard nach Ctrl+C unverändert/leer" erkannt; falsch-negativ, wenn
  die Selektion dem vorherigen Clipboard-Inhalt entspricht — seltener
  Edge-Case. (Linux/PRIMARY ist davon nicht betroffen.)
- **Wayland — `append`/`prepend`:** Die Kollaps-Pfeiltaste wird auf
  Wayland noch nicht über libei gesendet; dort landen diese Aktionen am
  Cursor (wie `replace`). `replace`/`insert` funktionieren. Offener
  Folgeschritt: Einzeltasten-`KeyCommand` im libei-Worker.

## macOS — nicht im Scope

Alle macOS-Implementierungen sind Stubs hinter
`#[cfg(target_os = "macos")]`. Der Code kompiliert dort, aber ein
funktionaler macOS-Port (CGEvent für Inject, NSStatusItem für Tray,
TCC-/Accessibility-Permissions, signierter `.dmg`) ist nicht eingeplant.

## Distribution-Bundles

`pnpm tauri build` produziert auf Linux drei Bundle-Formate. Wichtig:
der erste Release-Build dauert ~10–15 min (auf langsameren Systemen
deutlich mehr — der Compile von `whisper-rs-sys` mit cmake/clang-LTO
ist der Engpass), danach ist alles im Cargo-Release-Cache und folgende
Builds laufen in ~3–5 min.

**Voraussetzungen auf dem Build-System (Debian/Ubuntu):**
- alle Pakete aus dem Build-Anforderungen-Abschnitt oben
- zusätzlich `rpm` (stellt `rpmbuild` bereit) — sonst wird das
  RPM-Target ohne Fehler übersprungen

```bash
sudo apt-get install rpm
pnpm tauri build
```

**Output-Pfade nach erfolgreichem Build:**

```
src-tauri/target/release/bundle/deb/VoiceTypeX_0.1.0_amd64.deb         (~5 MB)
src-tauri/target/release/bundle/appimage/VoiceTypeX_0.1.0_amd64.AppImage  (~110 MB)
src-tauri/target/release/bundle/rpm/VoiceTypeX-0.1.0-1.x86_64.rpm      (~5 MB)
```

Der NSIS-Installer wird auf Linux übersprungen (NSIS-Toolchain ist
Windows-spezifisch) — kein Fehler, das ist erwartet.

### `.deb` installieren (Debian / Ubuntu / Linux Mint)

```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/VoiceTypeX_0.1.0_amd64.deb
# Bei fehlenden Deps:
sudo apt-get -f install
```

Nach der Installation erscheint *VoiceTypeX* im App-Menü. Start
über Menü oder `voicetypex` im Terminal.

Uninstall: `sudo apt remove voice-type-x` (Tauri normalisiert
`identifier` auf einen kebab-case-Paketnamen). User-Daten und
Keychain-Einträge bleiben dabei liegen — Cleanup siehe Abschnitt
*„Deinstallation — vollständige Spurenbeseitigung"* weiter unten.

### `.rpm` installieren (Fedora / RHEL / openSUSE)

RPM auf das Ziel-System kopieren (z.B. via `scp`, USB-Stick), dann:

```bash
sudo dnf install ./VoiceTypeX-0.1.0-1.x86_64.rpm
# Oder klassisch:
sudo rpm -i VoiceTypeX-0.1.0-1.x86_64.rpm
```

Uninstall: `sudo dnf remove voice-type-x`. User-Daten bleiben liegen
— siehe *„Deinstallation"*.

### AppImage starten (universell Linux)

Keine Installation nötig — `chmod +x`, dann doppelklicken oder im
Terminal:

```bash
chmod +x VoiceTypeX_0.1.0_amd64.AppImage
./VoiceTypeX_0.1.0_amd64.AppImage
```

Falls FUSE auf dem System fehlt oder deaktiviert ist:

```bash
./VoiceTypeX_0.1.0_amd64.AppImage --appimage-extract-and-run
```

Das AppImage enthält den kompletten GTK/WebKit-Stack — funktioniert
auf jeder modernen Linux-Distro, integriert sich aber **nicht** ins
App-Menü. Für dauerhafte Nutzung empfiehlt sich DEB oder RPM.

### Runtime-Dependencies (was die Pakete verlangen)

- **`.deb`** (von Tauri's Bundler ermittelt): `libopenblas0`,
  `libasound2`, `libxdo3`, `libayatana-appindicator3-1`,
  `libwebkit2gtk-4.1-0`, `libgtk-3-0` — alle aus dem Debian-Standard-
  Repo.
- **`.rpm`** (von Tauri's Bundler ermittelt): `openblas-serial`,
  `alsa-lib`, `libxdo`, `libayatana-appindicator3.so.1`,
  `libwebkit2gtk-4.1.so.0`, `libgtk-3.so.0` — alle aus dem
  Fedora-Standard-Repo.
- **AppImage**: nichts — alles eingebacken, ~110 MB.

## Deinstallation — vollständige Spurenbeseitigung

Der OS-Paket-Manager (apt/dnf/NSIS) entfernt nur das, was er installiert
hat. **Bewusst liegen** bleiben User-Daten, OS-Keychain-Einträge,
Autostart-Konfiguration und Wayland-Portal-Permissions — damit ein
Re-Install den User-Zustand wiederfindet.

### Was wo liegt

| Plattform | Pfad | Inhalt |
|---|---|---|
| Linux | `~/.config/de.kevin-stenzel.voicetypex/settings.json` | App-Einstellungen |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/secrets.json` (chmod 0600) | Cloud-API-Keys (Source of Truth) |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/wayland_session.json` (chmod 0600) | Wayland-Permission-Restore-Token |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/modes/*.toml` | Eigene + Default-Modi |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/models/` | Whisper-/VAD-/GGUF-Modelle (bis zu ~10 GB) |
| Linux | `~/.config/autostart/*VoiceType*.desktop` | Autostart-Eintrag (falls aktiviert) |
| Linux | gnome-keyring / kwallet, `service="voicetypex"` | API-Key-Mirror |
| Windows | `%APPDATA%\de.kevin-stenzel.voicetypex\config\` | Settings, Modi, Secrets, Token |
| Windows | `%APPDATA%\de.kevin-stenzel.voicetypex\data\` | Modelle |
| Windows | `%LocalAppData%\de.kevin-stenzel.voicetypex\EBWebView\` | WebView2-Profil-Cache |
| Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\VoiceTypeX` | Autostart-Registry (falls aktiviert) |
| Windows | Credential Manager, target `<provider>.voicetypex` (z.B. `xai.voicetypex`) | API-Key-Mirror |

### Drei Wege, das aufzuräumen

**1. In der App — vor dem Uninstall:**
*Einstellungen → Gefahrenzone* bietet drei Reset-Stufen
(API-Keys, Wayland-Token, Werksreset). Modelle bleiben dabei
unangetastet — separates Cache-Management in derselben Settings-Seite.

**2. Cleanup-Skript — nach dem Uninstall:**

Linux/macOS:
```bash
bash scripts/uninstall-cleanup.sh
```

Windows (PowerShell, *nicht* als Admin):
```powershell
powershell -ExecutionPolicy Bypass -File scripts\uninstall-cleanup.ps1
```

Beide Skripte sind interaktiv. Sie räumen User-Daten, OS-Keychain-
Einträge und Autostart-Eintrag weg, jeweils mit separater
Bestätigung. Voraussetzung auf Linux ist optional `libsecret-tools`
(Paket `libsecret-tools` auf Debian/Ubuntu) für die Keyring-
Löschung; ohne das gibt das Skript eine Anleitung für seahorse /
kwalletmanager aus.

**3. Manuell — Spuren, die kein Skript anfasst:**

- **Wayland-Portal-Permissions** (RemoteDesktop / GlobalShortcuts):
  - KDE Plasma 6: *System-Settings → Apps → Anwendungsberechtigungen
    → „Tastendrücke senden"* → VoiceTypeX entfernen.
    Analog für *„Globale Verknüpfungen"*.
  - GNOME: `gsettings list-recursively | grep desktop-portal`
    bzw. dconf-editor unter `/org/gnome/desktop-portal-permissions/`.
- **WebView2-State** (Windows): `%LocalAppData%\de.kevin-stenzel.
  voicetypex\EBWebView\` manuell löschen.
- **NSIS-Uninstaller-Eintrag** (Windows): über *Win+R → appwiz.cpl*.

## CI

GitHub Actions baut auf jedem Push/PR (`.github/workflows/ci.yml`):
- Linux (ubuntu-24.04) — `cargo fmt + clippy + test`, `pnpm lint + build`
- Windows (windows-latest) — `cargo check + test`, `pnpm build`
- Supply-Chain-Audit (`cargo audit`, `pnpm audit`)

Auf Tags `v*` baut `release.yml` via `tauri-action` für beide
Plattformen die Bundle-Artefakte (deb/rpm/AppImage/nsis), signiert die
Updater-Artefakte und legt ein GitHub-Release (Draft) mit Assets +
`latest.json` an.

> Der native Vulkan-GPU-Build auf den gehosteten Runnern (whisper.cpp /
> llama.cpp) muss beim ersten realen CI-Lauf validiert werden — die
> gepinnte Vulkan-SDK-Version ist die Schlüsselvariable.
