// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { hasEstimateLink } from "./estimateLink.js";

describe("hasEstimateLink", () => {
  it("reads a linked system as on", () => {
    expect(hasEstimateLink({ estimate: "623e4567-e89b-12d3-a456-426614174005" })).toBe(true);
  });

  it("reads a cleared link as off", () => {
    expect(hasEstimateLink({ estimate: null })).toBe(false);
  });

  it("reads an absent column as off, so list rows never enable estimates", () => {
    expect(hasEstimateLink({})).toBe(false);
  });

  it("reads a missing row as off", () => {
    expect(hasEstimateLink(undefined)).toBe(false);
    expect(hasEstimateLink(null)).toBe(false);
  });
});
