// SPDX-License-Identifier: GPL-3.0-or-later
import type { Config } from "tailwindcss";

// Tokens come as CSS custom properties from src/styles/globals.css
// (Light under :root, Dark under html.dark). The mapping function returns
// rgb(var(...) / <alpha-value>) so that Tailwind's opacity
// modifiers (bg-canvas/80) keep working.
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
      // Additional smaller size for supporting text (sidebar subtitle,
      // menu hotkey hint, mode description in the menu, initials badge).
      // 11px lower bound per readability recommendation; all former
      // text-[10px]/[11px] magic values fall back to it.
      fontSize: {
        xxs: ["11px", "14px"],
      },
    },
  },
  plugins: [],
};

export default config;
