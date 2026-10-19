// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    // Contract suites drive a live Django over HTTP (each write round-trips
    // the broker for activity tasks), so the 5s default flakes under
    // parallel load. Unit tests stay milliseconds-fast either way.
    testTimeout: 30_000,
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "json-summary", "lcov"],
      reportsDirectory: "coverage",
      include: ["src/**"],
    },
  },
});
