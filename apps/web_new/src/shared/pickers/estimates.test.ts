// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import type { Estimate } from "@pidash/api-client";
import { NO_ESTIMATE_ID, selectEstimateOptions } from "./estimates.js";

const POINTS_SYSTEM: Estimate = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  workspace: "723e4567-e89b-12d3-a456-426614174006",
  project: "823e4567-e89b-12d3-a456-426614174007",
  name: "Points",
  description: "",
  type: "points",
  last_used: true,
  points: [
    { id: "223e4567-e89b-12d3-a456-426614174001", key: 1, value: "1", description: "" },
    { id: "323e4567-e89b-12d3-a456-426614174002", key: 2, value: "3", description: "" },
  ],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  created_by: null,
  updated_by: null,
};

const TIME_SYSTEM: Estimate = {
  ...POINTS_SYSTEM,
  id: "423e4567-e89b-12d3-a456-426614174003",
  name: "Time",
  type: "time",
  points: [
    { id: "523e4567-e89b-12d3-a456-426614174004", key: 1, value: "30", description: "" },
    { id: "623e4567-e89b-12d3-a456-426614174005", key: 2, value: "90", description: "" },
  ],
};

describe("selectEstimateOptions", () => {
  it("leads with the clear row, then the points raw", () => {
    expect(selectEstimateOptions(POINTS_SYSTEM)).toEqual([
      { id: NO_ESTIMATE_ID, label: "No estimate", clear: true, disabled: false },
      { id: "223e4567-e89b-12d3-a456-426614174001", label: "1", clear: false, disabled: false },
      { id: "323e4567-e89b-12d3-a456-426614174002", label: "3", clear: false, disabled: false },
    ]);
    expect(NO_ESTIMATE_ID).toBe("none");
  });

  it("renders TIME points as hours and minutes", () => {
    expect(selectEstimateOptions(TIME_SYSTEM).map((option) => option.label)).toEqual(["No estimate", "30m", "1h 30m"]);
  });

  it("renders a single disabled row without an active system", () => {
    expect(selectEstimateOptions(undefined)).toEqual([
      { id: NO_ESTIMATE_ID, label: "No estimate", clear: true, disabled: true },
    ]);
  });
});
