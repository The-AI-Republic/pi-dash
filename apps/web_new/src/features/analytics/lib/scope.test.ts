// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics scope tests (parity/specs/analytics.md §4, ANL-004).
import { describe, expect, it } from "vitest";

import {
  ALL_PROJECTS_LABEL,
  MANY_PROJECTS_LABEL,
  analyticsScopeSearch,
  normalizeAnalyticsScope,
  parseAnalyticsSearch,
  resolveAnalyticsScope,
  selectAnalyticsProjects,
  selectPickerSummary,
  type ScopeProject,
} from "./scope.js";

function project(overrides: Partial<ScopeProject> & { id: string }): ScopeProject {
  return { name: `Project ${overrides.id}`, member_role: 15, archived_at: null, ...overrides };
}

describe("normalizeAnalyticsScope", () => {
  it("drops blanks and duplicates while preserving order", () => {
    expect(normalizeAnalyticsScope(["b", "", "a", "b", "  ", "c", "a"])).toEqual(["b", "a", "c"]);
  });

  it("keeps an empty selection empty", () => {
    expect(normalizeAnalyticsScope([])).toEqual([]);
  });
});

describe("selectAnalyticsProjects", () => {
  it("keeps joined, unarchived projects only", () => {
    const joined = project({ id: "p1" });
    expect(
      selectAnalyticsProjects([
        joined,
        project({ id: "p2", member_role: null }),
        project({ id: "p3", archived_at: "2024-05-06T07:08:09Z" }),
      ])
    ).toEqual([joined]);
  });
});

describe("resolveAnalyticsScope", () => {
  it("normalizes and drops ids the caller cannot resolve", () => {
    const projects = [project({ id: "a" }), project({ id: "b" })];
    expect(resolveAnalyticsScope(["b", "gone", "a", "b"], projects)).toEqual(["b", "a"]);
  });

  it("resolves to empty when nothing is selectable", () => {
    expect(resolveAnalyticsScope(["a"], [project({ id: "a", member_role: null })])).toEqual([]);
  });
});

describe("parseAnalyticsSearch", () => {
  it("accepts one comma-separated value", () => {
    expect(parseAnalyticsSearch({ project_ids: "a,b,a" })).toEqual({ project_ids: ["a", "b"] });
  });

  it("accepts a repeated parameter", () => {
    expect(parseAnalyticsSearch({ project_ids: ["a", "b,c"] })).toEqual({ project_ids: ["a", "b", "c"] });
  });

  it("ignores missing, blank, and mistyped parameters", () => {
    expect(parseAnalyticsSearch({})).toEqual({});
    expect(parseAnalyticsSearch({ project_ids: "" })).toEqual({});
    expect(parseAnalyticsSearch({ project_ids: " , " })).toEqual({});
    expect(parseAnalyticsSearch({ project_ids: 42 })).toEqual({});
    expect(parseAnalyticsSearch({ project_ids: ["a", 42, null] })).toEqual({ project_ids: ["a"] });
  });
});

describe("analyticsScopeSearch", () => {
  it("serializes a selection and clears on empty", () => {
    expect(analyticsScopeSearch(["a", "b"])).toEqual({ project_ids: ["a", "b"] });
    expect(analyticsScopeSearch([])).toEqual({});
  });
});

describe("selectPickerSummary", () => {
  it("labels the all-projects state", () => {
    expect(selectPickerSummary([])).toBe("All projects");
    expect(ALL_PROJECTS_LABEL).toBe("All projects");
  });

  it("reads one selection by name", () => {
    expect(selectPickerSummary(["Beta"])).toBe("Beta");
  });

  it("joins two or three selections", () => {
    expect(selectPickerSummary(["Alpha", "Beta"])).toBe("Alpha, Beta");
    expect(selectPickerSummary(["Alpha", "Beta", "Gamma"])).toBe("Alpha, Beta, Gamma");
  });

  it("compresses four or more selections into the fixed literal", () => {
    expect(selectPickerSummary(["a", "b", "c", "d"])).toBe("3+ projects");
    expect(selectPickerSummary(["a", "b", "c", "d", "e"])).toBe("3+ projects");
    expect(MANY_PROJECTS_LABEL).toBe("3+ projects");
  });
});
