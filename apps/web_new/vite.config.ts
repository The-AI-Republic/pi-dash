// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const target = process.env.PIDASH_TARGET === "desktop" ? "desktop" : "web";
const here = dirname(fileURLToPath(import.meta.url));

// Static SPA only (D9). One bundle per target; the desktop bundle is loaded
// by the Tauri shell. No file swapping: platform differences live behind
// core/platform (F-04), selected here at build time.
export default defineConfig(({ mode }) => {
  const buildTarget = mode === "desktop" || target === "desktop" ? "desktop" : "web";
  return {
    define: {
      __PIDASH_TARGET__: JSON.stringify(buildTarget),
    },
    outDir: `dist/${buildTarget}`,
    resolve: {
      alias: {
        // Resolve the workspace client to source so dev/test/typecheck
        // never depend on a prior package build; CI builds it anyway.
        "@pidash/api-client": resolve(here, "../../packages/api-client/src/index.ts"),
        // Single edition swap point: the cloud build replaces this target.
        "@pidash/edition": resolve(here, "src/core/edition/oss.ts"),
        // Build-time platform selection: only one implementation is ever
        // bundled, so the web bundle cannot contain desktop code.
        "@pidash/platform-target":
          buildTarget === "desktop"
            ? resolve(here, "src/core/platform/tauri.ts")
            : resolve(here, "src/core/platform/web.ts"),
      },
    },
    plugins: [
      TanStackRouterVite({ target: "react", autoCodeSplitting: true }),
      react({
        babel: {
          plugins: [["babel-plugin-react-compiler", {}]],
        },
      }),
      tailwindcss(),
    ],
    server: {
      port: 3010,
      strictPort: true,
    },
    build: {
      outDir: `dist/${buildTarget}`,
      emptyOutDir: true,
      sourcemap: false,
      chunkSizeWarningLimit: 200,
    },
  };
});
