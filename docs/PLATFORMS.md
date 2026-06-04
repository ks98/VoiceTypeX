# Platform Notes

## Linux

### Wayland (KDE Plasma 6, GNOME 46+)

Functionally complete: hotkeys via `xdg-desktop-portal.GlobalShortcuts`
(through `ashpd`), auto-paste via `xdg-desktop-portal.RemoteDesktop` +
`reis` (libei). On the first dictation the compositor shows a
permission dialog *"VoiceTypeX wants to send keystrokes"*; once granted,
the `restore_token` is persisted to
`~/.config/.../wayland_session.json` (chmod 0600) — no permission
dialog on subsequent app starts.

**Minimum versions:**
- `xdg-desktop-portal` ≥ 1.18
- `libei` (system library) ≥ 1.0
- KDE Plasma ≥ 6.1 (`xdg-desktop-portal-kde` with MR !223 + KWin MR !5496
  merged) **or** GNOME ≥ 46 (Mutter MR !2628 merged)

**Wayland focus quirks** (KDE Plasma 6). In short:
- The main window starts hidden (`visible:false`) — otherwise it gets
  focused on the hotkey trigger and steals focus from the target app.
- The overlay window is explicitly `hide()`-ed before the libei inject
  (with an 80 ms pause) so that focus jumps back to the target app.
- Consequence + workaround: because the main window is first mapped only
  later (via the tray), its WM close (X) button is dead on Wayland until
  the first `configure` event (tao 0.35.3, tauri#13440 — still open
  upstream). Worked around in `src-tauri/src/tray/mod.rs`
  (`reveal_main_window`): on Linux, after each tray `show()`, the window is
  briefly `maximize()`-d then `unmaximize()`-d (deferred so the fresh map
  settles first), which fires the `configure` that binds the close
  affordance — the same maximize→restore a user would otherwise do by hand.
  Runs on every reveal because each hide→show is a fresh GTK map. Drop once
  upstream ships a fix.

**Terminal paste (Ctrl+Shift+V).** Terminals (Konsole, …) paste on
`Ctrl+Shift+V`, not `Ctrl+V`, so the default auto-paste fails in them. With
`paste_shortcut = "auto"` (mode default) the app detects the focused window's
class on KDE Plasma 6 via a bundled KWin script
(`src-tauri/src/injection/focus_tracker.rs`): the script reports
`activeWindow.resourceClass` over a zbus D-Bus service
(`de.kevin_stenzel.voicetypex`), the app caches the last *foreign* class
(ignoring its own windows by PID) and sends `Ctrl+Shift+V` for terminal
classes, `Ctrl+V` otherwise. KDE-only; off KDE (or on KWin/D-Bus setup
failure) `auto` stays `Ctrl+V`, and the per-mode `paste_shortcut =
"ctrl_shift_v"` is the manual fallback. The detected class is logged
(Logs tab) for debugging.

### X11

Functionally complete: hotkeys via `tauri-plugin-global-shortcut`
(XGrabKey-based), clipboard inject via
`tauri-plugin-clipboard-manager` + a simulated `Ctrl+V` through `enigo`
(XTest).

### Hyprland / Sway / wlroots

`xdg-desktop-portal-hyprland` does not implement the `RemoteDesktop`
portal (issue #252 is open); the wlroots maintainers have taken a stance
against the portal approach. On these compositors the Wayland path falls
back to clipboard + a notification *"Press Ctrl+V"*. A native
auto-paste solution would have to be built on top of
`wlr-virtual-keyboard-unstable-v1` (e.g. via a `wtype` sub-process) —
currently **not implemented**, and with no concrete need, not planned
either.

### Build requirements (Debian/Ubuntu)

```bash
sudo apt-get install -y \
    build-essential pkg-config curl \
    libgtk-3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev \
    libayatana-appindicator3-dev librsvg2-dev \
    libssl-dev \
    libdbus-1-dev                # keyring -> dbus-secret-service (KEK for secrets.json at-rest) \
    libasound2-dev               # cpal (audio capture) \
    libxdo-dev                   # enigo (X11 keystroke; not runtime-relevant on Wayland) \
    libclang-dev cmake           # whisper-rs (bindgen + whisper.cpp build) \
    libvulkan-dev                # gpu-vulkan feature (Phase-3a default), headers + loader \
    glslc                        # GLSL->SPIR-V shader compiler, needed by whisper.cpp's Vulkan backend at build time \
    mesa-vulkan-drivers          # llvmpipe software-Vulkan fallback (systems without hardware Vulkan)
```

**Vulkan default (Phase 3a, May 2026):**
The default build uses the `gpu-vulkan` feature of `whisper-rs`.
Vulkan covers iGPU + AMD + Intel Arc + NVIDIA with a *single* binary
(~95% of consumer hardware). If no Vulkan device is available at
runtime, whisper.cpp falls back to CPU transparently — no app code
path needed.

`glslc` is build-time-only: whisper.cpp compiles its GLSL shaders to
SPIR-V during the cargo build. At runtime only `libvulkan1` + the
GPU driver are required.

If you don't want Vulkan (server/container/headless), be explicit:
```bash
cargo build --no-default-features --features custom-protocol,fast-cpu
```
The `fast-cpu` feature links OpenBLAS instead of Vulkan. Build
prerequisite for it: `libopenblas-dev` and `BLAS_INCLUDE_DIRS` set (see
below).

**Phase 3b — llama-cpp-sys-2 0.1.146 build quirk (automated):**
llama-cpp-sys-2 0.1.146's build.rs has a TOC/TOU bug — `Path::
exists()` follows symlinks and returns false for dangling links, but
`std::fs::hard_link()` then fails because the symlink entry is still
there. The result without a workaround: an `Os { code: 17, kind:
AlreadyExists }` panic on rebuild.

**Automated via an npm hook:** `scripts/clean-dangling-libs.mjs`
runs as `predev` and `prebuild` (see `package.json`) before every
`pnpm tauri dev` and `pnpm tauri build`. Pure Node, cross-platform,
no-op on Windows. If you invoke Cargo directly (e.g. `cargo build`
for tests), either use the Tauri workflow or run it manually:
```bash
node scripts/clean-dangling-libs.mjs
```

**Phase 3b — `dynamic-link` runtime expectations + bundle pipeline:**
llama-cpp-2 is linked with the `dynamic-link` feature; this produces
`libllama.so`, `libggml.so`, `libggml-cpu.so`, `libggml-vulkan.so`
and `libggml-base.so` as separate shared libs.

- **Dev build (`pnpm tauri dev`):** Cargo places the libs in
  `target/debug/` and sets the rpath there automatically. The binary
  finds them at runtime without any extra configuration.
- **Distribution build (`pnpm tauri build`):**
  1. Cargo builds the binary + libs in `target/release/`.
  2. `beforeBundleCommand` triggers `scripts/bundle-libs.mjs`, which
     copies the libs to `src-tauri/resources/lib/`.
  3. The `tauri.conf.json` entry `bundle.resources: ["resources/lib/*"]`
     packs them into the final bundle.
  4. `src-tauri/build.rs` sets the rpath so the dynamic linker finds the
     libs at runtime. Two specifics (verified against a real .rpm
     install, see below):
       * deb/rpm install `bundle.resources` under `/usr/lib/<productName>/`,
         i.e. `/usr/lib/VoiceTypeX/resources/lib/` — the verbatim
         productName, NOT the binary name `voicetypex`. The primary rpath
         entry is therefore `$ORIGIN/../lib/VoiceTypeX/resources/lib`;
         `$ORIGIN/../lib` (AppImage) and `$ORIGIN` (portable/dev) follow as
         fallbacks.
       * The libs have no rpath of their own and depend on each other
         transitively (llama -> ggml -> ggml-{cpu,vulkan,base}). Since
         DT_RUNPATH is not inherited by transitive deps, build.rs passes
         `-Wl,--disable-new-dtags` to emit DT_RPATH, which is.

`src-tauri/resources/lib/` is gitignored except for `.gitkeep` — its
contents are regenerated on every bundle build and don't belong in the
repo.

**Optional CUDA builder path (Task #27):**
Anyone building on a machine with the CUDA toolkit can additionally
enable the `embedded-cuda-dynamic` feature:
```bash
sudo apt install -y nvidia-cuda-toolkit  # or vendor download
cargo build --release --features embedded-cuda-dynamic
```
With this, llama-cpp-2 builds the GGML backends as separate shared libs
(`ggml-cpu.so`, `ggml-vulkan.so`, `ggml-cuda.so`); at runtime llama.cpp
picks the fastest available one. On user machines without a CUDA
driver the app falls back to Vulkan transparently — no code change
needed.

**Verified layout (v0.1.0 .rpm on Fedora 44).** `rpm -ql` + `readelf -d`
confirmed: binary at `/usr/bin/voicetypex`, libs at
`/usr/lib/VoiceTypeX/resources/lib/` (deb is identical — both use the
`productName` root). The very first v0.1.0 bundle failed to launch with
`error while loading shared libraries: libllama.so.0: cannot open shared
object file`: the original cascade (`$ORIGIN`, `$ORIGIN/lib`,
`$ORIGIN/../lib/voicetypex`, `$ORIGIN/../lib`) missed that path on two
counts (wrong case `voicetypex`, missing `/resources/lib`) and used
DT_RUNPATH, which never reaches the transitive `libggml*` deps. Fixed by
the DT_RPATH + productName-path rpath above. If a future Tauri bump
changes the install layout, re-check with `rpm -ql <pkg>` /
`dpkg-deb -c <deb>` and update build.rs accordingly.

**CPU ISA baseline (whisper.cpp / ggml) — avoid AVX-512 SIGILL.**
`whisper-rs-sys` defaults to `GGML_NATIVE=ON` (`-march=native`), so a build on
an AVX-512-capable CI runner bakes AVX-512 into whisper's *static* ggml and the
app dies with `Ungültiger Maschinenbefehl` / "illegal instruction" (SIGILL) on
consumer CPUs without AVX-512 — e.g. Intel Meteor Lake or any Alder-Lake+
consumer part (AVX-512 fused off). The CI builds therefore pin a portable
x86-64-v3 baseline via env vars that `whisper-rs-sys` forwards to ggml's CMake:
`GGML_NATIVE=OFF GGML_AVX=ON GGML_AVX2=ON GGML_FMA=ON GGML_F16C=ON` (set in
`release.yml` and `ci.yml`). These are **whisper-only**: `llama-cpp-sys-2`
forwards only `CMAKE_*` and derives `GGML_NATIVE` from the Rust `target-cpu`,
so it already builds portable and ignores these vars.

Trade-off: the binary now *requires* AVX2 (Haswell 2013+ / Zen+); pre-AVX2 CPUs
are unsupported. True runtime CPU dispatch (`GGML_CPU_ALL_VARIANTS`) needs a
shared-lib ggml + `GGML_BACKEND_DL`, which conflicts with whisper-rs-sys's
static `BUILD_SHARED_LIBS=OFF` — out of scope for now. NOTE: whisper-rs-sys has
no `rerun-if-env-changed` for `GGML_*`, so the `Swatinem/rust-cache`
`prefix-key` was bumped to force a clean rebuild; bump it again if you change
the baseline.

**BLAS_INCLUDE_DIRS (only for the `fast-cpu` feature):**
When `fast-cpu` is active, `whisper-rs-sys` 0.15+ needs
`BLAS_INCLUDE_DIRS` set explicitly. On Debian/Ubuntu the path is
`/usr/include/x86_64-linux-gnu/openblas-pthread` —
`src-tauri/.cargo/config.toml` sets this default with `force = false`.
On other distros set it manually:

```bash
# Fedora (libopenblas-devel):
export BLAS_INCLUDE_DIRS=/usr/include/openblas
# Arch (openblas):
export BLAS_INCLUDE_DIRS=/usr/include
```

**Wayland note:** `reis` is a pure-Rust implementation of the EI
protocol and needs no separate system library. It communicates
directly with the file descriptor that ashpd receives from the portal.
`xdg-desktop-portal-kde` or `xdg-desktop-portal-gnome` must be
installed as the backend (on KDE/GNOME usually the default).

On Fedora/Arch the package names are slightly different — the
principle list (GTK, WebKit2GTK 4.1, Soup3, AppIndicator, RSVG, ALSA,
libxdo, clang, cmake) stays the same.

### Known X11 limitations

- The paste shortcut on the clipboard path is fixed to `Ctrl+V`.
  Terminals often expect `Ctrl+Shift+V` — there the clipboard path
  pastes nothing. Workaround: set `injection_method = "keystrokes"` in
  the mode (see below — works on X11 and Windows; ignored on Wayland).
- Some WMs block `XGrabKey` for certain modifier combinations (e.g.
  when a WM shortcut already uses the same combo). In that case
  `tauri-plugin-global-shortcut` reports an error and VoiceTypeX shows
  a notification.

### Keystrokes mode (X11 + Windows)

Modes with `injection_method = "keystrokes"` bypass the clipboard
entirely. The text is typed character by character via `enigo.text(...)`
(Windows: `SendInput`, X11: `XTest`). Advantages: works in terminals
with `Ctrl+Shift+V` paste, in IME-sensitive apps, and in inputs with
clipboard blockers. Disadvantages: slower than paste, layout-dependent
— Unicode characters outside the active keyboard layout can fail.

**On Wayland** `keystrokes` currently falls back to the libei
clipboard path (with a hint log). Real keystroke injection via libei
would need char→keysym mapping through `xkbcommon` — not yet
implemented.

## Windows

Windows 10/11 with WebView2 (preinstalled on 11; on 10 it ships with
the Tauri installer).

### Build requirements

- Rust stable (`rustup` with the MSVC toolchain — recommended over GNU)
- Node.js 20+ and pnpm (easiest via `corepack enable`)
- Visual Studio Build Tools 2019+ with *"Desktop development with C++"*
- WebView2 Runtime (preinstalled on Win 11; otherwise from
  https://developer.microsoft.com/microsoft-edge/webview2/)

### Known Windows specifics

- `cargo` pulls in `whisper-rs-sys`, which compiles whisper.cpp's C++
  code with cmake/MSVC. First-time build ~5–10 min.
- **Vulkan SDK for build time** (Phase 3a default): install the
  LunarG Vulkan SDK (https://www.lunarg.com/vulkan-sdk/), the
  environment variable `VULKAN_SDK` must be set. Runtime: current GPU
  drivers ship `vulkan-1.dll` automatically (NVIDIA/AMD/Intel).
- **To build without Vulkan** (e.g. headless or license-strict):
  `cargo build --no-default-features --features custom-protocol,fast-cpu`.
  Then `BLAS_INCLUDE_DIRS` applies (an OpenBLAS Windows distribution is
  required, e.g. `set BLAS_INCLUDE_DIRS=C:\OpenBLAS\include`).
- `enigo` uses `SendInput` — works in most applications, but some
  UWP/WinUI apps have restrictive input paths. Workaround via the
  clipboard fallback (default).

### Windows — local LLM (embedded vs. Ollama)

The **embedded LLM** (`llama-cpp-2`) is **not** compiled on Windows.
Reason (ks98/VoiceTypeX#1): `whisper-rs-sys` and `llama-cpp-sys-2` each
bundle their own ggml; on MSVC the duplicate `ggml_*` symbols collide at
link time (LNK2005). Linux/ELF tolerates a symbol defined in both the
executable **and** a shared lib, MSVC does not. `llama-cpp-2` is
therefore target-gated in `Cargo.toml`
(`[target.'cfg(not(target_os = "windows"))'.dependencies]`),
and the `processing::embedded` path is `#[cfg(not(windows))]`.

**Consequence for the bundle:** On Windows `whisper-rs-sys` links its
ggml **statically** into the binary — so there is **no** `ggml-*.dll`/`llama.dll`
to bundle. `scripts/bundle-libs.mjs` is a no-op on Windows (it finds no
`llama.dll` marker and exits cleanly with exit 0). That also eliminates
the NSIS DLL loader issue noted here earlier entirely.

**The local LLM on Windows** runs via a **self-installed
Ollama daemon** (`local_engine = "ollama"`; the default engine value on
Windows is `"ollama"` instead of `"embedded"`) or via a **cloud
provider**. If a mode explicitly triggers `local_engine = "embedded"`,
the pipeline returns a clear error message instead of crashing. Speech
recognition (whisper.cpp + Vulkan) is unaffected by this and runs fully
locally.

| Platform | STT | Embedded LLM | Ollama LLM | Cloud LLM |
| --- | --- | --- | --- | --- |
| Linux / macOS | whisper.cpp (Vulkan) | ✅ llama-cpp-2 | ✅ | ✅ |
| Windows | whisper.cpp (Vulkan) | ❌ (#1) | ✅ (self-installed) | ✅ |

## Edit modes: reading the selection & writing it back

Modes with `input = "selection"` read the selected text of the focused
foreign app and write the result back. Both are platform-dependent and
belong to the paths that must be verified manually.

**Reading** (`TextInjector::read_selection`):

| Platform | Read mechanism | Status |
|---|---|---|
| Linux X11 | **PRIMARY selection** directly (arboard, native) | focus-independent, without Ctrl+C |
| Linux Wayland | **PRIMARY selection** via wlr/ext-data-control (arboard, feature `wayland-data-control`) | verified readable on KWin (even unfocused) |
| Windows | `enigo` Ctrl+C → save/read/restore clipboard | **to be verified manually** |

On Linux the selected text is automatically in the PRIMARY selection
(the "middle-click paste" buffer) — it is read directly, without
keyboard simulation, without focus, and without touching the normal
clipboard (CLIPBOARD). Windows has no PRIMARY selection, so the
simulated Ctrl+C route is still used there.

**Writing back** depends on `output`: `replace`/`insert` paste
directly (pasting over an active selection overwrites it);
`append`/`prepend` collapse the selection first via an arrow key
(right/left) and then paste.

### Manual verification (perform per platform)

1. Select text in a foreign app (an editor **and** a browser text
   field — those are the most different).
2. Hotkey → choose the edit mode "Improve" → speak the instruction →
   stop. Expectation: the selected text is replaced by the result.
3. "Write a reply" on a selected paragraph: the original stays, the
   reply appears below it.
4. "Free edit": replace vs. append depending on the instruction
   (`@@` control line).
5. Select nothing, hotkey, edit mode: PRIMARY is empty → empty
   selection (log: `Eager selection capture captured=false`).

### Known limitations & risks

- **PRIMARY semantics (Linux):** PRIMARY holds the *most recently
  selected* text — regardless of which window. If the user hasn't
  selected anything fresh, a stale/foreign selection gets read. In
  practice this is uncritical because the workflow is "select →
  immediately hotkey".
- **Apps without PRIMARY (Linux):** Some Chromium/Electron builds and
  a few terminals don't fill PRIMARY reliably → `read_selection`
  returns `None`, and the edit mode then operates on an empty
  selection.
- **`append`/`prepend` need the surviving selection (output):** Writing
  back collapses the selection via an arrow key. Apps that discard the
  selection on focus change (menu → target app) make the collapse run
  into nothing. Reading is not affected by this (PRIMARY persists
  independently of focus).
- **Windows `read_selection` heuristic:** "nothing selected" is
  detected via "clipboard unchanged/empty after Ctrl+C"; false-negative
  if the selection matches the previous clipboard content — a rare
  edge case. (Linux/PRIMARY is not affected by this.)
- **Wayland — `append`/`prepend`:** The collapse arrow key is not yet
  sent over libei on Wayland; there these actions land at the cursor
  (like `replace`). `replace`/`insert` work. Open follow-up:
  single-key `KeyCommand` in the libei worker.

## macOS — out of scope

All macOS implementations are stubs behind
`#[cfg(target_os = "macos")]`. The code compiles there, but a
functional macOS port (CGEvent for inject, NSStatusItem for the tray,
TCC/Accessibility permissions, a signed `.dmg`) is not planned.

## Distribution bundles

`pnpm tauri build` produces three bundle formats on Linux. Important:
the first release build takes ~10–15 min (significantly more on slower
systems — compiling `whisper-rs-sys` with cmake/clang LTO is the
bottleneck); after that everything is in the Cargo release cache and
subsequent builds run in ~3–5 min.

**Prerequisites on the build system (Debian/Ubuntu):**
- all packages from the build requirements section above
- additionally `rpm` (provides `rpmbuild`) — otherwise the RPM target
  is skipped without an error

```bash
sudo apt-get install rpm
pnpm tauri build
```

**Output paths after a successful build:**

```
src-tauri/target/release/bundle/deb/VoiceTypeX_0.1.0_amd64.deb         (~5 MB)
src-tauri/target/release/bundle/appimage/VoiceTypeX_0.1.0_amd64.AppImage  (~110 MB)
src-tauri/target/release/bundle/rpm/VoiceTypeX-0.1.0-1.x86_64.rpm      (~5 MB)
```

The NSIS installer is skipped on Linux (the NSIS toolchain is
Windows-specific) — no error, that's expected.

### Installing the `.deb` (Debian / Ubuntu / Linux Mint)

```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/VoiceTypeX_0.1.0_amd64.deb
# If dependencies are missing:
sudo apt-get -f install
```

After installation *VoiceTypeX* appears in the app menu. Start it
via the menu or `voicetypex` in the terminal.

Uninstall: `sudo apt remove voice-type-x` (Tauri normalizes
`identifier` to a kebab-case package name). User data and
keychain entries are left behind — for cleanup see the section
*"Uninstall — complete trace removal"* further below.

### Installing the `.rpm` (Fedora / RHEL / openSUSE)

Copy the RPM to the target system (e.g. via `scp`, USB stick), then:

```bash
sudo dnf install ./VoiceTypeX-0.1.0-1.x86_64.rpm
# Or classically:
sudo rpm -i VoiceTypeX-0.1.0-1.x86_64.rpm
```

Uninstall: `sudo dnf remove voice-type-x`. User data is left behind
— see *"Uninstall"*.

### Running the AppImage (universal Linux)

No installation needed — `chmod +x`, then double-click or in the
terminal:

```bash
chmod +x VoiceTypeX_0.1.0_amd64.AppImage
./VoiceTypeX_0.1.0_amd64.AppImage
```

If FUSE is missing or disabled on the system:

```bash
./VoiceTypeX_0.1.0_amd64.AppImage --appimage-extract-and-run
```

The AppImage contains the complete GTK/WebKit stack — it works on
any modern Linux distro, but it does **not** integrate into the app
menu. For permanent use, DEB or RPM is recommended.

### Runtime dependencies (what the packages require)

- **`.deb`** (determined by Tauri's bundler): `libopenblas0`,
  `libasound2`, `libxdo3`, `libayatana-appindicator3-1`,
  `libwebkit2gtk-4.1-0`, `libgtk-3-0` — all from the standard Debian
  repo.
- **`.rpm`** (determined by Tauri's bundler): `openblas-serial`,
  `alsa-lib`, `libxdo`, `libayatana-appindicator3.so.1`,
  `libwebkit2gtk-4.1.so.0`, `libgtk-3.so.0` — all from the
  standard Fedora repo.
- **AppImage**: nothing — everything baked in, ~110 MB.

## Uninstall — complete trace removal

The OS package manager (apt/dnf/NSIS) only removes what it installed.
**Deliberately left behind** are user data, OS keychain entries,
autostart configuration and Wayland portal permissions — so that a
re-install finds the user's state again.

### What lives where

| Platform | Path | Content |
|---|---|---|
| Linux | `~/.config/de.kevin-stenzel.voicetypex/settings.json` | App settings |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/secrets.json` (chmod 0600) | Cloud API keys (source of truth) |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/wayland_session.json` (chmod 0600) | Wayland permission restore token |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/modes/*.toml` | Custom + default modes |
| Linux | `~/.config/de.kevin-stenzel.voicetypex/models/` | Whisper/VAD/GGUF models (up to ~10 GB) |
| Linux | `~/.config/autostart/*VoiceType*.desktop` | Autostart entry (if enabled) |
| Linux | gnome-keyring / kwallet, `service="voicetypex"` | API key mirror |
| Windows | `%APPDATA%\de.kevin-stenzel.voicetypex\config\` | Settings, modes, secrets, token |
| Windows | `%APPDATA%\de.kevin-stenzel.voicetypex\data\` | Models |
| Windows | `%LocalAppData%\de.kevin-stenzel.voicetypex\EBWebView\` | WebView2 profile cache |
| Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\VoiceTypeX` | Autostart registry (if enabled) |
| Windows | Credential Manager, target `<provider>.voicetypex` (e.g. `xai.voicetypex`) | API key mirror |

### Three ways to clean this up

**1. In the app — before uninstalling:**
*Settings → Danger Zone* offers three reset levels
(API keys, Wayland token, factory reset). Models stay untouched
in the process — separate cache management on the same settings page.

**2. Cleanup script — after uninstalling:**

Linux/macOS:
```bash
bash scripts/uninstall-cleanup.sh
```

Windows (PowerShell, *not* as Admin):
```powershell
powershell -ExecutionPolicy Bypass -File scripts\uninstall-cleanup.ps1
```

Both scripts are interactive. They clean away user data, OS keychain
entries and the autostart entry, each with a separate confirmation.
The prerequisite on Linux is the optional `libsecret-tools` (package
`libsecret-tools` on Debian/Ubuntu) for the keyring deletion; without
it the script prints instructions for seahorse / kwalletmanager.

**3. Manually — traces no script touches:**

- **Wayland portal permissions** (RemoteDesktop / GlobalShortcuts):
  - KDE Plasma 6: *System Settings → Apps → Application Permissions
    → "Send keystrokes"* → remove VoiceTypeX.
    Same for *"Global Shortcuts"*.
  - GNOME: `gsettings list-recursively | grep desktop-portal`
    or dconf-editor under `/org/gnome/desktop-portal-permissions/`.
- **WebView2 state** (Windows): delete `%LocalAppData%\de.kevin-stenzel.
  voicetypex\EBWebView\` manually.
- **NSIS uninstaller entry** (Windows): via *Win+R → appwiz.cpl*.

## CI

GitHub Actions builds on every push/PR (`.github/workflows/ci.yml`):
- Linux (ubuntu-24.04) — `cargo fmt + clippy + test`, `pnpm lint + build`
- Windows (windows-latest) — `cargo build + test`, `pnpm build` (embedded
  llama-cpp-2 disabled, hence a full link instead of just `cargo check`)
- Supply-chain audit (`cargo audit`, `pnpm audit`)

On `v*` tags, `release.yml` builds the bundle artifacts
(deb/rpm/AppImage/nsis) for both platforms via `tauri-action`, signs
the updater artifacts and creates a GitHub release (draft) with assets +
`latest.json`.

> The native Vulkan GPU build on the hosted runners (whisper.cpp /
> llama.cpp) must be validated on the first real CI run — the pinned
> Vulkan SDK version is the key variable.
