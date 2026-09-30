// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
