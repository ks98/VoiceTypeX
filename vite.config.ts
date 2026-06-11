// SPDX-License-Identifier: GPL-3.0-or-later
import { configDefaults, defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
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
    outDir: "dist",
    sourcemap: false,
  },
  test: {
    // Keep the run scoped to the project: sibling worktrees under
    // .claude/worktrees/ hold stale test copies that otherwise inflate the count.
    exclude: [...configDefaults.exclude, "**/dist/**", "**/.claude/**"],
  },
});
