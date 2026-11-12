// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edition-only filter dimensions (ISS-234): toggling, counting, row
// matching and the applied chips the dropdown lists.
import { describe, expect, it } from "vitest";

import {
  appliedChips,
  editionFilterCount,
  emptyEditionFilters,
  matchesIssueType,
  matchesTeamProject,
  toggleEditionFilter,
} from "./filterModel.js";

const TEAM_A = "323e4567-e89b-12d3-a456-426614174002";
const TEAM_B = "423e4567-e89b-12d3-a456-426614174003";
const TYPE_A = "523e4567-e89b-12d3-a456-426614174004";

describe("toggleEditionFilter", () => {
  it("adds a value and remembers it", () => {
    const next = toggleEditionFilter(emptyEditionFilters(), "teamProject", TEAM_A);
    expect(next.teamProject).toEqual([TEAM_A]);
  });

  it("removes a value already applied", () => {
    const applied = { teamProject: [TEAM_A, TEAM_B], issueType: [] };
    expect(toggleEditionFilter(applied, "teamProject", TEAM_A).teamProject).toEqual([TEAM_B]);
  });

  it("leaves the other dimension alone", () => {
    const applied = { teamProject: [TEAM_A], issueType: [TYPE_A] };
    expect(toggleEditionFilter(applied, "teamProject", TEAM_A).issueType).toEqual([TYPE_A]);
  });
});

describe("editionFilterCount", () => {
  it("counts the dimensions carrying a value", () => {
    expect(editionFilterCount(emptyEditionFilters())).toBe(0);
    expect(editionFilterCount({ teamProject: [TEAM_A], issueType: [] })).toBe(1);
    expect(editionFilterCount({ teamProject: [TEAM_A], issueType: [TYPE_A] })).toBe(2);
  });
});

describe("row matching", () => {
  it("lets every team project through until one is applied", () => {
    expect(matchesTeamProject(TEAM_B, [])).toBe(true);
    expect(matchesTeamProject(TEAM_A, [TEAM_A])).toBe(true);
    expect(matchesTeamProject(TEAM_B, [TEAM_A])).toBe(false);
  });

  it("lets every type through until one is applied", () => {
    expect(matchesIssueType(null, [])).toBe(true);
    expect(matchesIssueType(TYPE_A, [TYPE_A])).toBe(true);
    expect(matchesIssueType(TYPE_A, ["other"])).toBe(false);
  });

  it("never matches an untyped row against a chosen type", () => {
    expect(matchesIssueType(null, [TYPE_A])).toBe(false);
  });
});

describe("appliedChips", () => {
  const options = {
    teamProject: [{ id: TEAM_A, name: "Atlas", identifier: "ATL" }],
    issueType: [{ id: TYPE_A, name: "Bug" }],
  };

  it("names each applied value from its option list, in dimension order", () => {
    expect(appliedChips({ teamProject: [TEAM_A], issueType: [TYPE_A] }, options)).toEqual([
      { key: "teamProject", value: TEAM_A, label: "Atlas" },
      { key: "issueType", value: TYPE_A, label: "Bug" },
    ]);
  });

  it("falls back to the id while an option list is still loading", () => {
    expect(appliedChips({ teamProject: [TEAM_B], issueType: ["unknown"] }, options)).toEqual([
      { key: "teamProject", value: TEAM_B, label: TEAM_B },
      { key: "issueType", value: "unknown", label: "unknown" },
    ]);
  });

  it("lists nothing with no filters applied", () => {
    expect(appliedChips(emptyEditionFilters(), options)).toEqual([]);
  });
});
