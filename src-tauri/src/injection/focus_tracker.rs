// SPDX-License-Identifier: GPL-3.0-or-later
//! KDE Plasma 6 (Wayland) terminal-window auto-detection.
//!
//! On Wayland a client cannot read the focused window's class itself. On KDE we
//! load a small KWin script that pushes the active window's `resourceClass` +
//! `pid` to us over D-Bus. We cache the last *foreign* class (ignoring our own
//! windows by PID) and classify whether it is a terminal, so the paste path can
//! pick Ctrl+Shift+V (terminals) vs Ctrl+V (everything else).
//!
//! Linux-only (the `mod` declaration is `cfg`-gated). Returns `None` (with a
//! logged reason) when not on KDE/Wayland or when the D-Bus / KWin setup fails —
//! callers then fall back to plain Ctrl+V. The manual per-mode
//! `paste_shortcut = "ctrl_shift_v"` always works regardless of this tracker.
//!
//! API correctness (KWin 6 scripting, the `org.kde.KWin` Scripting D-Bus
//! interface, zbus 5.15) was verified against KWin master source + the local
//! zbus crate; runtime behaviour on a live KDE session is validated by the user.

use std::process;
use std::sync::{Arc, RwLock};

use zbus::connection::{self, Connection};

/// Well-known bus name + object the KWin script calls back into. D-Bus name
/// elements may not contain `-`, so the app id's hyphen becomes `_`.
const APP_BUS_NAME: &str = "de.kevin_stenzel.voicetypex";
const FOCUS_OBJECT_PATH: &str = "/de/kevin_stenzel/voicetypex/FocusTracker";

/// KWin Scripting endpoints (org.kde.KWin / org.kde.kwin.Scripting).
const KWIN_BUS_NAME: &str = "org.kde.KWin";
const KWIN_SCRIPTING_PATH: &str = "/Scripting";
const KWIN_SCRIPTING_IFACE: &str = "org.kde.kwin.Scripting";
/// Stable plugin name we register the script under, so `unloadScript()` (which
/// matches strictly by name, not id) is deterministic across runs/crashes.
const KWIN_PLUGIN_NAME: &str = "de.kevin_stenzel.voicetypex.focustracker";

/// Substrings (matched case-insensitively against `resourceClass`) that mark a
/// terminal emulator. These are WM_CLASS / resourceClass values, not binaries.
const TERMINAL_CLASSES: &[&str] = &[
    "konsole",
    "yakuake",
    "gnome-terminal",
    "org.gnome.console",
    "kgx",
    "ptyxis",
    "xterm",
    "alacritty",
    "kitty",
    "foot",
    "wezterm",
    "tilix",
    "terminator",
    "contour",
    "rio",
    "urxvt",
    "qterminal",
    "deepin-terminal",
    "blackbox",
    "cool-retro-term",
    "hyper",
];

/// The KWin script source — written to a temp file and handed to KWin's
/// `loadScript`. On every window activation (and once at start) it pushes the
/// active window's `resourceClass` + `pid` to our D-Bus method.
const KWIN_SCRIPT_JS: &str = r#"// SPDX-License-Identifier: GPL-3.0-or-later
function reportWindow(window) {
    if (!window) { return; }
    var resourceClass = window.resourceClass ? String(window.resourceClass) : "";
    var pid = (typeof window.pid === "number") ? window.pid : -1;
    callDBus(
        "de.kevin_stenzel.voicetypex",
        "/de/kevin_stenzel/voicetypex/FocusTracker",
        "de.kevin_stenzel.voicetypex.FocusTracker",
        "WindowActivated",
        resourceClass,
        pid
    );
}
workspace.windowActivated.connect(function (window) { reportWindow(window); });
reportWindow(workspace.activeWindow);
"#;

/// Shared cache of the last foreign window class. `None` == no detection yet.
/// `std::sync::RwLock` so the sync `focused_is_terminal()` reads it reliably;
/// the writer holds the guard only for a tiny, await-free critical section.
type ClassCache = Arc<RwLock<Option<String>>>;

/// D-Bus interface served at `FOCUS_OBJECT_PATH`. Holds the cache + our own PID
/// so it can ignore activations of our own windows (overlay/menu/main).
struct FocusTrackerInterface {
    cache: ClassCache,
    own_pid: i32,
}

#[zbus::interface(name = "de.kevin_stenzel.voicetypex.FocusTracker")]
impl FocusTrackerInterface {
    /// Called by the KWin script on every window activation.
    #[zbus(name = "WindowActivated")]
    async fn window_activated(&self, resource_class: String, pid: i32) {
        if pid == self.own_pid {
            tracing::debug!(
                class = %resource_class,
                pid,
                "FocusTracker: ignored (own window)"
            );
            return;
        }
        if let Ok(mut guard) = self.cache.write() {
            *guard = Some(resource_class.clone());
        }
        tracing::info!(
            class = %resource_class,
            pid,
            "FocusTracker: stored foreign window class"
        );
    }
}

/// Public handle. Keeps the zbus connection (and thus the object server) alive
/// for the app's lifetime, and remembers the loaded KWin script for cleanup.
pub struct KdeFocusTracker {
    cache: ClassCache,
    /// Kept alive so the object server keeps serving; also used for cleanup.
    connection: Connection,
    script_id: i32,
}

impl KdeFocusTracker {
    /// Whether the currently-focused window is a terminal. Empty cache (no
    /// detection yet, or read poisoned) -> `false` -> safe Ctrl+V default.
    pub fn focused_is_terminal(&self) -> bool {
        let class = self.cache.read().ok().and_then(|g| g.clone());
        match class {
            None => {
                tracing::debug!("FocusTracker: no class cached -> not terminal (default)");
                false
            }
            Some(c) => {
                let lc = c.to_lowercase();
                let is_term = TERMINAL_CLASSES.iter().any(|t| lc.contains(t));
                tracing::debug!(class = %c, is_terminal = is_term, "FocusTracker: classify");
                is_term
            }
        }
    }
}

/// Start the KDE focus tracker. Returns `None` (with a logged reason) when not
/// applicable so the caller can fall back to plain Ctrl+V. Async so the caller
/// drives it on the existing tokio runtime (`tauri::async_runtime::block_on`).
pub async fn start() -> Option<Arc<KdeFocusTracker>> {
    // --- Environment gating (boundary validation) ------------------------
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if !desktop.to_uppercase().split(':').any(|d| d == "KDE") {
        tracing::info!(xdg_current_desktop = %desktop, "FocusTracker: not KDE -> disabled");
        return None;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        tracing::info!("FocusTracker: no WAYLAND_DISPLAY (not Wayland) -> disabled");
        return None;
    }

    let cache: ClassCache = Arc::new(RwLock::new(None));
    let own_pid = process::id() as i32;

    match setup_async(cache.clone(), own_pid).await {
        Ok((connection, script_id)) => {
            tracing::info!(
                script_id,
                plugin = KWIN_PLUGIN_NAME,
                "FocusTracker: KWin script loaded and running"
            );
            Some(Arc::new(KdeFocusTracker {
                cache,
                connection,
                script_id,
            }))
        }
        Err(e) => {
            tracing::warn!(error = %e, "FocusTracker: setup failed -> disabled (Ctrl+V fallback)");
            None
        }
    }
}

/// Serve the FocusTracker interface, claim the bus name, write+load+run the
/// KWin script. Returns the live connection (must stay alive) and the script id.
async fn setup_async(
    cache: ClassCache,
    own_pid: i32,
) -> Result<(Connection, i32), Box<dyn std::error::Error + Send + Sync>> {
    let iface = FocusTrackerInterface { cache, own_pid };

    // Open the session connection, serve our object, claim the well-known name.
    // Keeping `connection` alive keeps the object server serving.
    let connection = connection::Builder::session()?
        .name(APP_BUS_NAME)?
        .serve_at(FOCUS_OBJECT_PATH, iface)?
        .build()
        .await?;

    tracing::info!(
        bus_name = APP_BUS_NAME,
        path = FOCUS_OBJECT_PATH,
        "FocusTracker: serving D-Bus object"
    );

    let script_path = write_script_tempfile()?;
    let script_path_str = script_path.to_string_lossy().into_owned();

    // Clear any stale instance left loaded by a previous run (our Drop unload
    // is best-effort and usually cannot run at process exit). Ignoring the
    // result: "not loaded" is the normal first-run case. This also avoids the
    // loadScript == -1 ("already loaded") path entirely.
    let _ = unload_script(&connection).await;

    let script_id = load_script(&connection, &script_path_str).await?;
    tracing::info!(script_id, path = %script_path_str, "FocusTracker: KWin loadScript returned");
    if script_id < 0 {
        return Err(format!("KWin loadScript returned {script_id} after unload").into());
    }

    // Run the loaded script. KWin 6 changed the per-script object path — the
    // KWin-5-era `/{id}` no longer exists ("No such object path '/0'"), so we
    // use `org.kde.kwin.Scripting.start()` on /Scripting, which runs all loaded
    // scripts and is path-independent across KWin versions.
    start_scripts(&connection).await?;
    Ok((connection, script_id))
}

async fn load_script(
    connection: &Connection,
    path: &str,
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let reply = connection
        .call_method(
            Some(KWIN_BUS_NAME),
            KWIN_SCRIPTING_PATH,
            Some(KWIN_SCRIPTING_IFACE),
            "loadScript",
            &(path, KWIN_PLUGIN_NAME),
        )
        .await?;
    Ok(reply.body().deserialize()?)
}

/// Run all loaded KWin scripts (including ours) via
/// `org.kde.kwin.Scripting.start()` on `/Scripting`. Version-stable, unlike the
/// per-script `/{id}` object whose path changed in KWin 6.
async fn start_scripts(
    connection: &Connection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    connection
        .call_method(
            Some(KWIN_BUS_NAME),
            KWIN_SCRIPTING_PATH,
            Some(KWIN_SCRIPTING_IFACE),
            "start",
            &(),
        )
        .await?;
    tracing::debug!("FocusTracker: KWin Scripting.start() invoked");
    Ok(())
}

async fn unload_script(
    connection: &Connection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    connection
        .call_method(
            Some(KWIN_BUS_NAME),
            KWIN_SCRIPTING_PATH,
            Some(KWIN_SCRIPTING_IFACE),
            "unloadScript",
            &(KWIN_PLUGIN_NAME,),
        )
        .await?;
    Ok(())
}

/// Write the embedded KWin JS to a per-user, per-process file (KWin reads it at
/// load time). Kept for the process lifetime so the retry path still finds it.
///
/// Security: the file is one KWin executes, so it must not be world-readable or
/// pre-creatable by another local user. We place it under `$XDG_RUNTIME_DIR`
/// (per-user, 0700 by spec) inside a `0700` subdir, and create the file itself
/// `0600` so it never exists with umask-default perms. When `$XDG_RUNTIME_DIR`
/// is unset we fall back to the world-readable `temp_dir()` — the 0600 file
/// perms still keep the contents private there.
fn write_script_tempfile() -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.push("voicetypex");
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;

    let mut path = dir;
    path.push(format!("focustracker-{}.js", process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?;
    std::io::Write::write_all(&mut file, KWIN_SCRIPT_JS.as_bytes())?;
    Ok(path)
}

impl Drop for KdeFocusTracker {
    fn drop(&mut self) {
        // Best-effort unload by plugin name. Drop is sync, call_method is async,
        // so spawn a detached task on the tokio runtime via a cheap Connection
        // clone. If no runtime is current (app teardown), skip — the next start
        // unloads+reloads via the -1 retry path anyway.
        let connection = self.connection.clone();
        let script_id = self.script_id;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                match unload_script(&connection).await {
                    Ok(()) => {
                        tracing::info!(script_id, "FocusTracker: unloaded KWin script on drop")
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "FocusTracker: unload on drop failed")
                    }
                }
            });
        }
    }
}
