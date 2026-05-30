// SPDX-License-Identifier: GPL-3.0-or-later
import React from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import App from "./App";
import Menu from "./views/Menu";
import Overlay from "./views/Overlay";
import { initTheme, subscribeSystemTheme } from "./lib/theme";
import { ipcGetSettings } from "./lib/tauri";
import { pickSupported, useI18nStore } from "./i18n";
import "./styles/globals.css";

// Set the theme synchronously BEFORE React render — otherwise the
// wrong theme briefly flickers on mount ("FOUC"). subscribeSystemTheme
// reacts to OS theme changes, but only when the user choice is
// "system".
initTheme();
subscribeSystemTheme(() => {
  // Re-apply already happens in the listener itself; this is just a
  // hook for future store synchronization, if needed.
});

const rootEl = document.getElementById("root");
if (!rootEl) {
  throw new Error(
    "Root-Element #root nicht gefunden — index.html beschaedigt?",
  );
}

// Window routing: Tauri starts three windows from the same
// index.html (label=main → App, label=overlay → Overlay, label=menu
// → Menu). The distinction is made via URL query, since Tauri
// forwards the `url` field of the window config cleanly to the
// renderer URL.
const params = new URLSearchParams(window.location.search);
const win = params.get("window");

const view =
  win === "overlay" ? <Overlay /> : win === "menu" ? <Menu /> : <App />;

// i18n bootstrap BEFORE React render: fetches Settings.locale from the
// backend and sets the store. The backend detected and persisted the
// OS locale on the first app start (see lib.rs::run), so we usually
// get a concrete value here. IPC errors are logged; the render happens
// in any case (otherwise the app would hang on backend crashes).
//
// Promise chain instead of top-level await: the build target is es2021
// (vite.config.ts), and TLA only landed in ES2022. Tauri keeps the
// WebView requirements deliberately conservative.
// Cross-window locale sync: every webview window (main, overlay, menu)
// subscribes to "i18n://locale-changed". When the user switches the
// language in settings, the event is emitted and all three stores are
// updated locally. The subscriber is registered BEFORE the render so
// that even a tightly-timed simultaneous event is not lost.
void listen<{ locale: string }>("i18n://locale-changed", (event) => {
  const next = pickSupported(event.payload.locale);
  useI18nStore.setState({ locale: next });
});

ipcGetSettings()
  .then((settings) => {
    const picked = pickSupported(settings.locale);
    // Visible in the diagnostics log: bug reports "app is on the
    // wrong language" can only be debugged when the bootstrap path
    // shows up in the log.
    console.info(
      `i18n bootstrap: raw="${settings.locale ?? "<null>"}" picked="${picked}"`,
    );
    useI18nStore.setState({ locale: picked });
  })
  .catch((e) => {
    console.warn("i18n bootstrap failed — rendering with default locale:", e);
  })
  .finally(() => {
    ReactDOM.createRoot(rootEl).render(
      <React.StrictMode>{view}</React.StrictMode>,
    );
  });
