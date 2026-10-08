// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared setup for web_new component tests: jest-dom matchers and DOM
// cleanup between tests (mirrors @pidash/kit).
import "@testing-library/jest-dom/vitest";
import * as matchers from "@testing-library/jest-dom/matchers";
import { cleanup } from "@testing-library/react";
import { afterEach, expect } from "vitest";

expect.extend(matchers);

afterEach(() => {
  // Node-env suites never mount; only jsdom suites have a DOM to clean.
  if (typeof globalThis.document !== "undefined") {
    cleanup();
  }
});
