// SPDX-License-Identifier: GPL-3.0-or-later
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Menu from "./views/Menu";
import Overlay from "./views/Overlay";
import "./styles/globals.css";

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

ReactDOM.createRoot(rootEl).render(
  <React.StrictMode>{view}</React.StrictMode>,
);
