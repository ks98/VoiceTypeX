// SPDX-License-Identifier: GPL-3.0-or-later
import { configDefaults, defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
    ...(host
      ? {
          hmr: {
            protocol: "ws",
            host,
            port: 1421,
          },
        }
      : {}),
  },
  build: {
    target: "es2021",
    // Tailwind 4 relies on cascade layers, @property and color-mix(); the
    // es2021-derived CSS target (Safari 14.1) would have Lightning CSS
    // lower for engines that lack them. These are Tailwind's minimums —
    // WebView2 is evergreen, the supported WebKitGTK releases are newer.
    cssTarget: ["chrome111", "safari16.4"],
    outDir: "dist",
    sourcemap: false,
  },
  test: {
    // Keep the run scoped to the project: sibling worktrees under
    // .claude/worktrees/ hold stale test copies that otherwise inflate the count.
    exclude: [...configDefaults.exclude, "**/dist/**", "**/.claude/**"],
  },
});
