// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import config from "../../vitest.config.js";
import { FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY, FULL_RUN_MAX_WORKERS } from "./testRunConcurrency.js";

describe("test run concurrency", () => {
  it("bounds the worker pool below Vitest's cpus - 1 default", () => {
    expect(FULL_RUN_MAX_WORKERS).toBeGreaterThan(0);
    expect(FULL_RUN_MAX_WORKERS).toBeLessThanOrEqual(4);
    expect(config.test?.maxWorkers).toBe(FULL_RUN_MAX_WORKERS);
  });

  it("bounds v8 coverage processing below Vitest's min(20, cpus) default", () => {
    expect(FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY).toBeGreaterThan(0);
    expect(FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY).toBeLessThanOrEqual(4);
    expect(config.test?.coverage?.processingConcurrency).toBe(FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY);
  });
});
