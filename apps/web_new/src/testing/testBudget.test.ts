// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import config from "../../vitest.config.js";
import { FULL_RUN_OBSERVED_LOAD, FULL_RUN_OBSERVED_P999_MS, FULL_RUN_TEST_TIMEOUT_MS } from "./testBudget.js";

describe("full-run test budget", () => {
  it("leaves headroom over the measured loaded p99.9", () => {
    expect(FULL_RUN_TEST_TIMEOUT_MS).toBeGreaterThanOrEqual(2 * FULL_RUN_OBSERVED_P999_MS);
  });

  it("stays bounded so a hung test still fails promptly", () => {
    expect(FULL_RUN_TEST_TIMEOUT_MS).toBeLessThanOrEqual(60_000);
  });

  it("records the machine load the evidence was taken under", () => {
    expect(FULL_RUN_OBSERVED_LOAD).toBeGreaterThan(0);
    expect(FULL_RUN_OBSERVED_P999_MS).toBeLessThan(FULL_RUN_TEST_TIMEOUT_MS);
  });

  it("is the timeout the vitest config runs with", () => {
    expect(config.test?.testTimeout).toBe(FULL_RUN_TEST_TIMEOUT_MS);
    expect(config.test?.hookTimeout).toBe(FULL_RUN_TEST_TIMEOUT_MS);
  });
});
