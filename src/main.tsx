// SPDX-License-Identifier: GPL-3.0-or-later
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Menu from "./views/Menu";
import Overlay from "./views/Overlay";
import { initTheme, subscribeSystemTheme } from "./lib/theme";
import { ipcGetSettings } from "./lib/tauri";
import { pickSupported, useI18nStore } from "./i18n";
import "./styles/globals.css";

// Theme synchron VOR React-Render setzen — sonst flackert beim Mount
// kurz das falsche Theme ("FOUC"). subscribeSystemTheme reagiert auf
// OS-Theme-Wechsel, aber nur wenn die User-Wahl "system" ist.
initTheme();
subscribeSystemTheme(() => {
  // Re-apply ist bereits im Listener selbst; hier nur als Hook für
  // zukünftige Store-Synchronisation, falls nötig.
});

const rootEl = document.getElementById("root");
if (!rootEl) {
  throw new Error(
    "Root-Element #root nicht gefunden — index.html beschaedigt?",
  );
}

// Window-Routing: Tauri startet drei Fenster aus derselben index.html
// (label=main → App, label=overlay → Overlay, label=menu → Menu). Die
// Unterscheidung erfolgt per URL-Query, da Tauri das `url`-Field der
// Window-Config sauber an die Renderer-URL weiterreicht.
const params = new URLSearchParams(window.location.search);
const win = params.get("window");

const view =
  win === "overlay" ? <Overlay /> : win === "menu" ? <Menu /> : <App />;

// i18n-Bootstrap VOR React-Render: holt Settings.locale aus dem Backend
// und setzt den Store. Backend hat beim ersten App-Start die OS-Locale
// detected und persistiert (siehe lib.rs::run), darum bekommen wir hier
// idR. einen konkreten Wert. IPC-Fehler werden geloggt; Render erfolgt
// in jedem Fall (sonst haengt die App bei Backend-Crashes).
//
// Promise-Chain statt top-level-await: das Build-Target ist es2021
// (vite.config.ts), und TLA ist erst ES2022. Tauri haelt die WebView-
// Anforderungen bewusst konservativ.
ipcGetSettings()
  .then((settings) => {
    const picked = pickSupported(settings.locale);
    // Sichtbar im Diagnostics-Log: Bug-Reports "App ist auf falscher
    // Sprache" lassen sich nur debuggen, wenn man den Bootstrap-Pfad
    // im Log sieht.
    console.info(
      `i18n bootstrap: raw="${settings.locale ?? "<null>"}" picked="${picked}"`,
    );
    useI18nStore.setState({ locale: picked });
  })
  .catch((e) => {
    console.warn(
      "i18n bootstrap failed — rendering with default locale:",
      e,
    );
  })
  .finally(() => {
    ReactDOM.createRoot(rootEl).render(
      <React.StrictMode>{view}</React.StrictMode>,
    );
  });
