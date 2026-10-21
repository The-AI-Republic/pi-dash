// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates model (PSET-049, EST-005/006/009/010/012): active and
// superseded selection, value labels, the OSS starter sets and the
// per-value rules.
import type { Estimate } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  CUSTOM_STARTER_ID,
  ESTIMATE_FAMILIES,
  ESTIMATE_VALUE_MAX_LENGTH,
  estimateStarters,
  familySystemName,
  renumberValues,
  selectActiveSystem,
  selectSupersededSystems,
  starterValues,
  systemValueLabels,
  valueSetProblems,
  VALUE_DUPLICATE,
  VALUE_NOT_CATEGORY,
  VALUE_NOT_NUMBER,
  VALUE_REQUIRED,
  VALUE_TOO_LONG,
} from "./model.js";

function estimate(overrides: Partial<Estimate> & { id: string }): Estimate {
  return {
    workspace: "11111111-1111-4111-8111-111111111111",
    project: "22222222-2222-4222-8222-222222222222",
    name: "System",
    description: "",
    type: "points",
    last_used: false,
    points: [],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

describe("estimate families", () => {
  it("offers points and categories only in this build", () => {
    expect(ESTIMATE_FAMILIES.map((family) => family.value)).toEqual(["points", "categories"]);
    expect(ESTIMATE_FAMILIES.map((family) => family.label)).toEqual(["Points", "Categories"]);
  });

  it("names the created system after its family", () => {
    expect(familySystemName("points")).toBe("Points");
    expect(familySystemName("categories")).toBe("Categories");
  });
});

describe("starters", () => {
  it("puts the scratch card first with the family's minimal values", () => {
    const points = estimateStarters("points");
    expect(points[0]).toEqual({ id: CUSTOM_STARTER_ID, title: "Custom", values: ["1", "2"] });
    expect(points.map((starter) => starter.title)).toEqual(["Custom", "Fibonacci", "Linear", "Squares"]);
    expect(estimateStarters("categories")[0]?.values).toEqual(["Easy", "Hard"]);
  });

  it("previews each template's values", () => {
    const byTitle = new Map(estimateStarters("points").map((starter) => [starter.title, starter.values]));
    expect(byTitle.get("Fibonacci")).toEqual(["1", "2", "3", "5", "8", "13"]);
    expect(byTitle.get("Linear")).toEqual(["1", "2", "3", "4", "5", "6"]);
    expect(byTitle.get("Squares")).toEqual(["1", "4", "9", "16", "25", "36"]);
    expect(estimateStarters("categories").find((starter) => starter.title === "T-Shirt Sizes")?.values).toEqual([
      "XS",
      "S",
      "M",
      "L",
      "XL",
      "XXL",
    ]);
  });

  it("resolves values by id and refuses an unknown one", () => {
    expect(starterValues("points", "linear")).toEqual(["1", "2", "3", "4", "5", "6"]);
    expect(starterValues("points", "missing")).toBeUndefined();
  });
});

describe("active and superseded systems", () => {
  it("picks the system marked in use", () => {
    const active = estimate({ id: "a", last_used: true });
    expect(selectActiveSystem([estimate({ id: "b" }), active])).toBe(active);
    expect(selectActiveSystem([estimate({ id: "b" })])).toBeUndefined();
    expect(selectActiveSystem([])).toBeUndefined();
  });

  it("lists superseded systems newest first", () => {
    const older = estimate({ id: "old", created_at: "2023-01-01T00:00:00Z" });
    const newer = estimate({ id: "new", created_at: "2024-06-01T00:00:00Z" });
    const active = estimate({ id: "live", last_used: true, created_at: "2025-01-01T00:00:00Z" });
    expect(selectSupersededSystems([older, active, newer]).map((row) => row.id)).toEqual(["new", "old"]);
  });

  it("labels values in position order and humanizes durations", () => {
    expect(
      systemValueLabels(
        estimate({
          id: "a",
          type: "time",
          points: [
            { id: "p3", key: 3, value: "150", description: "" },
            { id: "p1", key: 1, value: "30", description: "" },
            { id: "p2", key: 2, value: "60", description: "" },
          ],
        })
      )
    ).toEqual(["30m", "1h", "2h 30m"]);
    expect(
      systemValueLabels(
        estimate({
          id: "b",
          points: [
            { id: "p2", key: 2, value: "L", description: "" },
            { id: "p1", key: 1, value: "S", description: "" },
          ],
        })
      )
    ).toEqual(["S", "L"]);
  });
});

describe("value rules", () => {
  it("accepts positive numbers for the points family", () => {
    expect(valueSetProblems("points", ["1", "2", "13"])).toEqual([]);
  });

  it("refuses empty, non-positive, non-numeric, duplicate and over-long values", () => {
    const problems = valueSetProblems("points", ["", "0", "abc", "1", "1", "x".repeat(ESTIMATE_VALUE_MAX_LENGTH + 1)]);
    expect(problems).toEqual([
      { index: 0, message: VALUE_REQUIRED },
      { index: 1, message: VALUE_NOT_NUMBER },
      { index: 2, message: VALUE_NOT_NUMBER },
      { index: 4, message: VALUE_DUPLICATE },
      { index: 5, message: VALUE_TOO_LONG },
    ]);
  });

  it("ignores case when spotting duplicates", () => {
    expect(valueSetProblems("categories", ["Easy", "easy"])).toEqual([{ index: 1, message: VALUE_DUPLICATE }]);
  });

  it("refuses numbers in the categories family", () => {
    expect(valueSetProblems("categories", ["Easy", "2"])).toEqual([{ index: 1, message: VALUE_NOT_CATEGORY }]);
    expect(valueSetProblems("categories", ["XS", "S"])).toEqual([]);
  });

  it("trims before judging a value", () => {
    expect(valueSetProblems("points", ["  3  "])).toEqual([]);
    expect(valueSetProblems("points", ["   "])).toEqual([{ index: 0, message: VALUE_REQUIRED }]);
  });

  it("renumbers positions 1..n in display order", () => {
    expect(renumberValues(["b", "a"])).toEqual([
      { key: 1, value: "b" },
      { key: 2, value: "a" },
    ]);
  });
});
