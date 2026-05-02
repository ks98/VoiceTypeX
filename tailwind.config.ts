// SPDX-License-Identifier: GPL-3.0-or-later
import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        brand: {
          DEFAULT: "#5B21B6",
          50: "#F5F3FF",
          100: "#EDE9FE",
          500: "#8B5CF6",
          700: "#6D28D9",
          900: "#4C1D95",
        },
        status: {
          idle: "#94A3B8",
          recording: "#DC2626",
          processing: "#F59E0B",
          done: "#10B981",
          error: "#991B1B",
        },
      },
    },
  },
  plugins: [],
};

export default config;
