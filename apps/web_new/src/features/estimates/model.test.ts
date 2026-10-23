// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Estimates model (PSET-049, EST-005/006/009/010/012): active and
// superseded selection, value labels, family capability filtering, the
// starter sets and the per-value rules.
import type { Estimate } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  CUSTOM_STARTER_ID,
  ESTIMATE_FAMILIES,
  ESTIMATE_FAMILY_CATALOGUE,
  ESTIMATE_FAMILY_TIME,
  ESTIMATE_VALUE_MAX_LENGTH,
  estimateFamilies,
  estimateFamilyEnabled,
  estimatePreview,
  estimateStarters,
  estimateValueLabel,
  familySystemName,
  moveValueBefore,
  renumberValues,
  selectActiveSystem,
  selectSupersededSystems,
  starterValues,
  systemValueLabels,
  truncateEstimateValue,
  valueSetProblems,
  VALUE_DUPLICATE,
  VALUE_NOT_CATEGORY,
  VALUE_NOT_NUMBER,
  VALUE_NOT_POSITIVE,
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
  it("offers points and categories in the community edition", () => {
    expect(ESTIMATE_FAMILIES.map((family) => family.value)).toEqual(["points", "categories"]);
    expect(ESTIMATE_FAMILIES.map((family) => family.label)).toEqual(["Points", "Categories"]);
    expect(estimateFamilies({}).map((family) => family.value)).toEqual(["points", "categories"]);
  });

  it("adds time only when the edition capability is on", () => {
    expect(ESTIMATE_FAMILY_CATALOGUE.map((family) => family.value)).toEqual(["points", "categories", "time"]);
    expect(estimateFamilies({ "estimate-time-family": true }).map((family) => family.value)).toEqual([
      "points",
      "categories",
      "time",
    ]);
    expect(estimateFamilyEnabled(ESTIMATE_FAMILY_TIME, {})).toBe(false);
    expect(estimateFamilyEnabled(ESTIMATE_FAMILY_TIME, { "estimate-time-family": true })).toBe(true);
    expect(estimateFamilyEnabled("points", {})).toBe(true);
    expect(estimateFamilyEnabled("unknown" as never, { "estimate-time-family": true })).toBe(false);
  });

  it("names the created system after its family", () => {
    expect(familySystemName("points")).toBe("Points");
    expect(familySystemName("categories")).toBe("Categories");
    expect(familySystemName("time")).toBe("Time");
  });
});

describe("starters", () => {
  it("puts the scratch card first with the family's minimal values", () => {
    const points = estimateStarters("points");
    expect(points[0]).toEqual({ id: CUSTOM_STARTER_ID, title: "Custom", values: ["1", "2"] });
    expect(points.map((starter) => starter.title)).toEqual(["Custom", "Fibonacci", "Linear", "Squares"]);
    expect(estimateStarters("categories")[0]?.values).toEqual(["Easy", "Hard"]);
    expect(estimateStarters("time")[0]?.values).toEqual(["1", "2"]);
  });

  it("offers the category and time templates named by the spec", () => {
    expect(estimateStarters("categories").map((starter) => starter.title)).toEqual([
      "Custom",
      "T-Shirt Sizes",
      "Easy to hard",
    ]);
    expect(estimateStarters("time").map((starter) => starter.title)).toEqual(["Custom", "Hours"]);
    expect(estimateStarters("categories").find((starter) => starter.id === "easy-to-hard")?.values).toEqual([
      "Easy",
      "Medium",
      "Hard",
      "Very Hard",
    ]);
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

  it("humanizes time previews while keeping stored values raw", () => {
    const hours = estimateStarters("time").find((starter) => starter.title === "Hours");
    expect(hours?.values).toEqual(["1", "2", "3", "4", "5", "6"]);
    expect(estimatePreview("time", hours?.values ?? [])).toBe("1m, 2m, 3m, 4m, 5m, 6m");
    expect(estimatePreview("time", ["90", "150", "60", "45"])).toBe("1h 30m, 2h 30m, 1h, 45m");
    expect(estimateValueLabel("points", "90")).toBe("90");
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
    expect(valueSetProblems("time", ["1", "90", "150"])).toEqual([]);
  });

  it("refuses empty, non-numeric, non-positive and over-long values", () => {
    const problems = valueSetProblems("points", ["", "0", "abc", "x".repeat(ESTIMATE_VALUE_MAX_LENGTH + 1)]);
    expect(problems).toEqual([
      { index: 0, message: VALUE_REQUIRED },
      { index: 1, message: VALUE_NOT_POSITIVE },
      { index: 2, message: VALUE_NOT_NUMBER },
      { index: 3, message: VALUE_TOO_LONG },
    ]);
  });

  it("compares numeric duplicates by value", () => {
    expect(valueSetProblems("points", ["1", "1.0", "01"])).toEqual([{ index: 1, message: VALUE_DUPLICATE }]);
    expect(valueSetProblems("time", ["90", " 90 "])).toEqual([{ index: 1, message: VALUE_DUPLICATE }]);
  });

  it("compares category duplicates exactly and case-sensitively", () => {
    expect(valueSetProblems("categories", ["Easy", "easy"])).toEqual([]);
    expect(valueSetProblems("categories", ["Easy", "Easy"])).toEqual([{ index: 1, message: VALUE_DUPLICATE }]);
  });

  it("refuses numbers in the categories family", () => {
    expect(valueSetProblems("categories", ["Easy", "2"])).toEqual([{ index: 1, message: VALUE_NOT_CATEGORY }]);
    expect(valueSetProblems("categories", ["XS", "S"])).toEqual([]);
  });

  it("trims before judging a value", () => {
    expect(valueSetProblems("points", ["  3  "])).toEqual([]);
    expect(valueSetProblems("points", ["   "])).toEqual([{ index: 0, message: VALUE_REQUIRED }]);
  });

  it("truncates over-long edits to the server cap", () => {
    expect(ESTIMATE_VALUE_MAX_LENGTH).toBe(20);
    expect(truncateEstimateValue("1234567890123456789012345")).toBe("12345678901234567890");
  });

  it("renumbers positions 1..n in display order", () => {
    expect(renumberValues(["b", "a"])).toEqual([
      { key: 1, value: "b" },
      { key: 2, value: "a" },
    ]);
  });

  it("moves a value before a target and refuses no-op requests", () => {
    expect(moveValueBefore(["1", "2", "3"], 0, 2)).toEqual(["2", "1", "3"]);
    expect(moveValueBefore(["1", "2", "3"], 2, 0)).toEqual(["3", "1", "2"]);
    expect(moveValueBefore(["1", "2", "3"], 1, 1)).toEqual(["1", "2", "3"]);
    expect(moveValueBefore(["1", "2", "3"], -1, 1)).toEqual(["1", "2", "3"]);
    expect(moveValueBefore(["1", "2", "3"], 1, 4)).toEqual(["1", "2", "3"]);
    expect(moveValueBefore(["1", "2", "3"], 0, 2).length).toBe(3);
    expect(moveValueBefore(["1"], 0, 0)).toEqual(["1"]);
  });
});
