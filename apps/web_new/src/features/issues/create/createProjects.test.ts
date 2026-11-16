// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-selection rules the shell entry points share (ISS-117/ISS-123).
// The module stays free of editor and picker imports so registering `ni`
// does not pull the dialog into the shell's initial bundle.

import { describe, expect, it } from "vitest";

import {
  selectCreatableProjects,
  selectCreatableWorkItemProjects,
  TITLE_MAX_LENGTH,
  toCreateProjectOption,
  type CreateProjectOption,
} from "./createProjects.js";

function project(overrides: Partial<CreateProjectOption> = {}): CreateProjectOption {
  return { id: "p1", name: "Atlas", identifier: "ATL", memberRole: 15, archived: false, ...overrides };
}

describe("toCreateProjectOption", () => {
  it("derives the identifier from the name and fills the optional defaults", () => {
    expect(toCreateProjectOption({ id: "p1", name: "Quay" })).toEqual({
      id: "p1",
      name: "Quay",
      identifier: "QUA",
      memberRole: null,
      archived: false,
    });
  });

  it("keeps the caller's identifier, role and archive flag", () => {
    expect(
      toCreateProjectOption({ id: "p1", name: "Atlas", identifier: "ATL", memberRole: 20, archived: true })
    ).toEqual({ id: "p1", name: "Atlas", identifier: "ATL", memberRole: 20, archived: true });
  });
});

describe("selectCreatableProjects", () => {
  it("keeps members and admins and drops guests, outsiders and archived rows", () => {
    const rows = [
      project({ id: "member", memberRole: 15 }),
      project({ id: "admin", memberRole: 20 }),
      project({ id: "guest", memberRole: 5 }),
      project({ id: "outsider", memberRole: null }),
      project({ id: "archived", memberRole: 15, archived: true }),
    ];
    expect(selectCreatableProjects(rows).map((row) => row.id)).toEqual(["member", "admin"]);
  });

  it("returns an empty list for a guest-only collection", () => {
    expect(selectCreatableProjects([project({ memberRole: 5 })])).toEqual([]);
  });
});

describe("selectCreatableWorkItemProjects", () => {
  it("normalizes caller rows before filtering", () => {
    const rows = [
      { id: "p1", name: "Atlas", memberRole: 15 },
      { id: "p2", name: "Guestland", memberRole: 5 },
      { id: "p3", name: "Old", memberRole: 20, archived: true },
      { id: "p4", name: "Bare" },
      { id: "p5", name: "Deep Work", identifier: "DPW", memberRole: 20 },
    ];
    expect(selectCreatableWorkItemProjects(rows)).toEqual([
      { id: "p1", name: "Atlas", identifier: "ATL", memberRole: 15, archived: false },
      { id: "p5", name: "Deep Work", identifier: "DPW", memberRole: 20, archived: false },
    ]);
  });
});

describe("TITLE_MAX_LENGTH", () => {
  it("stays the API's 255-character cap", () => {
    expect(TITLE_MAX_LENGTH).toBe(255);
  });
});
