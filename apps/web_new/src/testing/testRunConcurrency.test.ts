// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import config from "../../vitest.config.js";
import { FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY } from "./testRunConcurrency.js";

describe("coverage run concurrency", () => {
  it("bounds v8 coverage processing below Vitest's min(20, cpus) default", () => {
    expect(FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY).toBeGreaterThan(0);
    expect(FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY).toBeLessThanOrEqual(4);
    expect(config.test?.coverage?.processingConcurrency).toBe(FULL_RUN_COVERAGE_PROCESSING_CONCURRENCY);
  });

  it("leaves the test worker pool at Vitest's default so runs keep their throughput", () => {
    expect(config.test?.maxWorkers).toBeUndefined();
  });
});
