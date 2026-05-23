// SPDX-License-Identifier: GPL-3.0-or-later
import type { Config } from "tailwindcss";

// Tokens kommen als CSS-Custom-Properties aus src/styles/globals.css
// (Light unter :root, Dark unter html.dark). Die Mapping-Funktion gibt
// rgb(var(...) / <alpha-value>) zurück, damit Tailwind's Opacity-
// Modifier (bg-canvas/80) weiter funktionieren.
const v = (name: string) => `rgb(var(--${name}-rgb) / <alpha-value>)`;

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        canvas: v("canvas"),
        surface: v("surface"),
        elevated: v("elevated"),
        outline: {
          DEFAULT: v("outline"),
          strong: v("outline-strong"),
        },
        fg: {
          DEFAULT: v("fg"),
          muted: v("fg-muted"),
          faint: v("fg-faint"),
        },
        brand: {
          DEFAULT: v("brand"),
          hover: v("brand-hover"),
          contrast: v("brand-contrast"),
        },
        status: {
          idle: v("status-idle"),
          recording: v("status-recording"),
          processing: v("status-processing"),
          done: v("status-done"),
          error: v("status-error"),
        },
      },
      fontFamily: {
        mono: [
          "JetBrains Mono",
          "SF Mono",
          "Cascadia Mono",
          "Roboto Mono",
          "Consolas",
          "Liberation Mono",
          "Menlo",
          "monospace",
        ],
      },
      // Zusätzliche kleinere Stufe für Begleittext (Sidebar-Subtitle,
      // Menu-Hotkey-Hint, Mode-Description im Menu, Initials-Badge).
      // 11px Lower-Bound nach Lesbarkeits-Empfehlung; alle ehemaligen
      // text-[10px]/[11px]-Magic-Werte fallen darauf.
      fontSize: {
        xxs: ["11px", "14px"],
      },
    },
  },
  plugins: [],
};

export default config;
