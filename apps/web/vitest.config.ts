/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import path from "path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "happy-dom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/**/*.test.ts", "tests/**/*.test.tsx"],
    css: false,
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./core"),
      "@/app": path.resolve(__dirname, "./app"),
      "@/helpers": path.resolve(__dirname, "./helpers"),
      "@/styles": path.resolve(__dirname, "./styles"),
      "@/pi-dash-web": path.resolve(__dirname, "./ce"),
    },
  },
});
