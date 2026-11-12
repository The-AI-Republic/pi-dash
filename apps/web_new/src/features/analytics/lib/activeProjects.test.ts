// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Active-projects list rule tests (parity/specs/analytics.md §7, ANL-007).
import { describe, expect, it } from "vitest";

import { completionPercent, completionTone, resolveActiveProjects } from "./activeProjects.js";

describe("completionPercent", () => {
  it("reads zero without a positive total", () => {
    expect(completionPercent(0, 0)).toBe(0);
    expect(completionPercent(0, 5)).toBe(0);
    expect(completionPercent(null, 1)).toBe(0);
    expect(completionPercent(undefined, 1)).toBe(0);
    expect(completionPercent(-2, 1)).toBe(0);
  });

  it("reads missing or non-positive completions as zero", () => {
    expect(completionPercent(4, null)).toBe(0);
    expect(completionPercent(4, undefined)).toBe(0);
    expect(completionPercent(4, 0)).toBe(0);
    expect(completionPercent(4, -1)).toBe(0);
  });

  it("rounds the completed share", () => {
    expect(completionPercent(2, 2)).toBe(100);
    expect(completionPercent(2, 1)).toBe(50);
    expect(completionPercent(3, 2)).toBe(67);
    expect(completionPercent(3, 1)).toBe(33);
    expect(completionPercent(1, 0)).toBe(0);
  });
});

describe("completionTone", () => {
  it("flips to success strictly above the halfway mark", () => {
    expect(completionTone(100)).toBe("success");
    expect(completionTone(51)).toBe("success");
    expect(completionTone(50)).toBe("danger");
    expect(completionTone(0)).toBe("danger");
  });
});

describe("resolveActiveProjects", () => {
  const projects = [
    {
      id: "a",
      name: "Alpha",
      identifier: "ALP",
      logo_props: {},
      member_role: 15,
      archived_at: null,
    },
    {
      id: "b",
      name: "Beta",
      identifier: "BET",
      logo_props: { emoji: "rocket" },
      member_role: 15,
      archived_at: null,
    },
  ];

  it("joins stats rows to projects in server order", () => {
    expect(
      resolveActiveProjects(
        [
          { id: "b", total_issues: 2, completed_issues: 1 },
          { id: "a", total_issues: 1, completed_issues: 1 },
        ],
        projects
      )
    ).toEqual([
      {
        id: "b",
        name: "Beta",
        identifier: "BET",
        logoProps: { emoji: "rocket" },
        total: 2,
        completed: 1,
      },
      { id: "a", name: "Alpha", identifier: "ALP", logoProps: {}, total: 1, completed: 1 },
    ]);
  });

  it("skips rows that no longer resolve and reads missing counts as zero", () => {
    expect(resolveActiveProjects([{ id: "stale", total_issues: 3, completed_issues: 3 }], projects)).toEqual([]);
    expect(resolveActiveProjects([{ id: "a" }], projects)).toEqual([
      { id: "a", name: "Alpha", identifier: "ALP", logoProps: {}, total: 0, completed: 0 },
    ]);
    expect(resolveActiveProjects([{ id: "a", total_issues: null, completed_issues: null }], projects)).toEqual([
      { id: "a", name: "Alpha", identifier: "ALP", logoProps: {}, total: 0, completed: 0 },
    ]);
  });

  it("joins no rows for no stats", () => {
    expect(resolveActiveProjects([], projects)).toEqual([]);
  });
});
