// SPDX-License-Identifier: GPL-3.0-or-later
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Overlay from "./views/Overlay";
import "./styles/globals.css";

const rootEl = document.getElementById("root");
if (!rootEl) {
  throw new Error(
    "Root-Element #root nicht gefunden — index.html beschaedigt?",
  );
}

// Window-Routing: Tauri startet zwei Fenster aus derselben index.html
// (label=main → App, label=overlay → Overlay). Die Unterscheidung erfolgt
// per URL-Query, da Tauri das `url`-Field der Window-Config sauber an
// die Renderer-URL weiterreicht.
const params = new URLSearchParams(window.location.search);
const isOverlay = params.get("window") === "overlay";

ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>{isOverlay ? <Overlay /> : <App />}</React.StrictMode>,
);
