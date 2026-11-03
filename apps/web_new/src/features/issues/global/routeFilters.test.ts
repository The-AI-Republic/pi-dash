// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { hasRouteFilters, parseRouteFilters } from "./routeFilters.js";

describe("parseRouteFilters", () => {
  it("keeps every string parameter, known or not", () => {
    expect(parseRouteFilters({ priority: "urgent", order_by: "-created_at", strange: "value" })).toEqual({
      priority: "urgent",
      order_by: "-created_at",
      strange: "value",
    });
  });

  it("drops absent values and keeps blanks", () => {
    expect(parseRouteFilters({ priority: undefined, state: null, label: "" })).toEqual({ label: "" });
  });

  it("keeps the last value of a repeated key", () => {
    expect(parseRouteFilters({ priority: ["urgent", "low"] })).toEqual({ priority: "low" });
  });

  it("ignores non-scalar entries", () => {
    expect(parseRouteFilters({ a: { b: 1 }, b: 2, c: [true, { d: 1 }] })).toEqual({ b: "2" });
  });

  it("returns an empty record for the plain view", () => {
    expect(parseRouteFilters({})).toEqual({});
  });
});

describe("hasRouteFilters", () => {
  it("reports presence by key count", () => {
    expect(hasRouteFilters({})).toBe(false);
    expect(hasRouteFilters({ priority: "urgent" })).toBe(true);
  });
});
