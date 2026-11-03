// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Project } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  compareSidebarOrder,
  isAggregateRowActive,
  isProjectsGroupRoute,
  isSidebarProject,
  isSubnavEntryActive,
  moveProjectToIndex,
  projectOrderPatch,
  resolveDropTarget,
  resolveProjectRowHref,
  resolveReorder,
  revealRow,
  selectSidebarProjects,
  selectSubnavEntries,
  sliceVisibleProjects,
  sortOrderForMove,
} from "./sidebar.js";
import { selectProjectTabs, type ProjectTabFlags } from "./tabs.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function project(id: string, overrides: Partial<Project> = {}): Project {
  return {
    id,
    name: `Project ${id}`,
    identifier: id.toUpperCase(),
    sort_order: null,
    logo_props: {},
    member_role: 20,
    intake_count: 0,
    archived_at: null,
    workspace: WS_ID,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 2,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    ...overrides,
  };
}

const FLAGS: ProjectTabFlags = {
  cycle_view: true,
  module_view: true,
  issue_views_view: true,
  page_view: true,
  inbox_view: true,
};

describe("isSidebarProject", () => {
  it("keeps joined, unarchived projects", () => {
    expect(isSidebarProject(project("a"))).toBe(true);
  });

  it("drops outsiders and archived projects", () => {
    expect(isSidebarProject(project("a", { member_role: null }))).toBe(false);
    expect(isSidebarProject(project("a", { archived_at: "2024-03-04T00:00:00Z" }))).toBe(false);
  });
});

describe("selectSidebarProjects", () => {
  it("filters to joined rows in ascending stored order", () => {
    const rows = selectSidebarProjects([
      project("b", { sort_order: 20000 }),
      project("outsider", { sort_order: 1, member_role: null }),
      project("a", { sort_order: 10000 }),
      project("old", { sort_order: 5, archived_at: "2024-03-04T00:00:00Z" }),
    ]);
    expect(rows.map((row) => row.id)).toEqual(["a", "b"]);
  });

  it("sorts unpositioned rows first and keeps ties stable", () => {
    const rows = selectSidebarProjects([
      project("b", { sort_order: 10000 }),
      project("fresh-b", { sort_order: null }),
      project("a", { sort_order: 10000 }),
      project("fresh-a", { sort_order: null }),
    ]);
    expect(rows.map((row) => row.id)).toEqual(["fresh-b", "fresh-a", "b", "a"]);
  });

  it("compares missing positions as below every stored one", () => {
    expect(compareSidebarOrder({ sort_order: null }, { sort_order: null })).toBe(0);
    expect(compareSidebarOrder({ sort_order: null }, { sort_order: 0 })).toBeLessThan(0);
    expect(compareSidebarOrder({ sort_order: 0 }, { sort_order: null })).toBeGreaterThan(0);
  });
});

describe("sliceVisibleProjects", () => {
  const rows = [project("a"), project("b"), project("c")];

  it("shows every row without a cap", () => {
    expect(sliceVisibleProjects(rows, null)).toEqual(rows);
  });

  it("shows the leading rows up to the cap", () => {
    expect(sliceVisibleProjects(rows, 2).map((row) => row.id)).toEqual(["a", "b"]);
  });

  it("shows nothing for a zero or negative cap", () => {
    expect(sliceVisibleProjects(rows, 0)).toEqual([]);
    expect(sliceVisibleProjects(rows, -3)).toEqual([]);
  });
});

describe("moveProjectToIndex", () => {
  it("moves the dragged row into its slot", () => {
    expect(moveProjectToIndex(["a", "b", "c"], "c", 0)).toEqual(["c", "a", "b"]);
    expect(moveProjectToIndex(["a", "b", "c"], "a", 1)).toEqual(["b", "a", "c"]);
    expect(moveProjectToIndex(["a", "b", "c"], "a", 2)).toEqual(["b", "c", "a"]);
  });

  it("clamps slots into the list", () => {
    expect(moveProjectToIndex(["a", "b"], "b", -4)).toEqual(["b", "a"]);
    expect(moveProjectToIndex(["a", "b"], "a", 9)).toEqual(["b", "a"]);
  });

  it("leaves unknown rows alone", () => {
    expect(moveProjectToIndex(["a", "b"], "missing", 0)).toEqual(["a", "b"]);
  });
});

describe("resolveDropTarget", () => {
  const rows = [
    { id: "a", top: 0, bottom: 30 },
    { id: "b", top: 30, bottom: 60 },
    { id: "c", top: 60, bottom: 90 },
  ];

  it("targets the first row the pointer has not cleared", () => {
    expect(resolveDropTarget(rows, 5, "c")).toEqual({ id: "a", after: false });
    expect(resolveDropTarget(rows, 40, "a")).toEqual({ id: "b", after: false });
  });

  it("targets past the last row below every middle", () => {
    expect(resolveDropTarget(rows, 80, "a")).toEqual({ id: "c", after: true });
    expect(resolveDropTarget(rows, 500, "a")).toEqual({ id: "c", after: true });
  });

  it("stays silent over the dragged row or an empty list", () => {
    expect(resolveDropTarget(rows, 5, "a")).toBeNull();
    expect(resolveDropTarget(rows, 500, "c")).toBeNull();
    expect(resolveDropTarget([], 10, "a")).toBeNull();
  });
});

describe("resolveReorder", () => {
  const ordered = [
    { id: "a", sort_order: 10000 },
    { id: "b", sort_order: 20000 },
    { id: "c", sort_order: 30000 },
  ];

  it("writes the dragged row before its target", () => {
    expect(resolveReorder(ordered, "c", { id: "a", after: false })).toEqual({
      projectId: "c",
      sort_order: 0,
    });
  });

  it("writes the dragged row after its target", () => {
    expect(resolveReorder(ordered, "a", { id: "c", after: true })).toEqual({
      projectId: "a",
      sort_order: 40000,
    });
    expect(resolveReorder(ordered, "c", { id: "a", after: true })).toEqual({
      projectId: "c",
      sort_order: 15000,
    });
  });

  it("stays silent when the drop changes nothing", () => {
    expect(resolveReorder(ordered, "a", null)).toBeNull();
    expect(resolveReorder(ordered, "a", { id: "a", after: false })).toBeNull();
    expect(resolveReorder(ordered, "a", { id: "b", after: false })).toBeNull();
  });
});

describe("revealRow", () => {
  it("scrolls the active row into view", () => {
    let seen: ScrollIntoViewOptions | undefined;
    revealRow({ scrollIntoView: (options) => (seen = options) }, true);
    expect(seen).toEqual({ block: "nearest" });
  });

  it("stays quiet offscreen-less, inactive or missing", () => {
    expect(() => revealRow(null, true)).not.toThrow();
    expect(() => revealRow({}, true)).not.toThrow();
    let calls = 0;
    revealRow(
      {
        scrollIntoView: () => {
          calls += 1;
        },
      },
      false
    );
    expect(calls).toBe(0);
  });
});

describe("sortOrderForMove", () => {
  const ordered = [
    { id: "a", sort_order: 10000 },
    { id: "b", sort_order: 20000 },
    { id: "c", sort_order: 30000 },
  ];

  it("jumps past the edge neighbor at either end", () => {
    expect(sortOrderForMove(ordered, "c", ["c", "a", "b"])).toBe(0);
    expect(sortOrderForMove(ordered, "a", ["b", "c", "a"])).toBe(40000);
  });

  it("lands midway between the new neighbors", () => {
    expect(sortOrderForMove(ordered, "c", ["a", "c", "b"])).toBe(15000);
  });

  it("reads missing positions as zero", () => {
    const unpositioned = [
      { id: "a", sort_order: null },
      { id: "b", sort_order: null },
    ];
    expect(sortOrderForMove(unpositioned, "b", ["b", "a"])).toBe(-10000);
    expect(sortOrderForMove(unpositioned, "a", ["b", "a"])).toBe(10000);
  });

  it("refuses drops outside the ordered rows", () => {
    expect(sortOrderForMove(ordered, "missing", ["a", "b", "c"])).toBeNull();
    expect(sortOrderForMove(ordered, "a", ["a", "b"])).toBeNull();
  });

  it("degrades missing neighbors to zero", () => {
    expect(sortOrderForMove([{ id: "a", sort_order: 5000 }], "a", ["a"])).toBe(-10000);
    expect(sortOrderForMove(ordered, "c", ["x", "c", "b"])).toBe(10000);
  });
});

describe("projectOrderPatch", () => {
  const ordered = [
    { id: "a", sort_order: 10000 },
    { id: "b", sort_order: 20000 },
  ];

  it("writes the dragged row's new position", () => {
    expect(projectOrderPatch(ordered, ["b", "a"], "b")).toEqual({ projectId: "b", sort_order: 0 });
  });

  it("stays silent when the drop changes nothing", () => {
    expect(projectOrderPatch(ordered, ["a", "b"], "b")).toBeNull();
    expect(projectOrderPatch(ordered, ["a"], "a")).toBeNull();
    expect(projectOrderPatch(ordered, ["b", "a"], "missing")).toBeNull();
  });
});

describe("selectSubnavEntries", () => {
  it("lists every tab plus workers for members, with slashed addresses", () => {
    const entries = selectSubnavEntries({
      workspaceSlug: "acme",
      projectId: "p1",
      flags: FLAGS,
      role: 20,
      intakeCount: 3,
    });
    expect(entries.map((entry) => entry.name)).toEqual([
      "Work Items",
      "Cycles",
      "Modules",
      "Views",
      "Pages",
      "Intake",
      "Schedulers",
      "AI Workers",
    ]);
    expect(entries[0]?.href).toBe("/acme/projects/p1/issues/");
    expect(entries[7]?.href).toBe("/acme/projects/p1/runners/");
    expect(entries[5]).toMatchObject({ key: "intake", badge: 3 });
  });

  it("hides flag-off and guest-hidden entries", () => {
    const entries = selectSubnavEntries({
      workspaceSlug: "acme",
      projectId: "p1",
      flags: { ...FLAGS, cycle_view: false, module_view: false, issue_views_view: false, page_view: true },
      role: 5,
      intakeCount: 0,
    });
    expect(entries.map((entry) => entry.name)).toEqual(["Work Items", "Pages", "Intake", "Schedulers"]);
    expect(entries.find((entry) => entry.key === "intake")?.badge).toBeNull();
    const outsider = selectSubnavEntries({
      workspaceSlug: "acme",
      projectId: "p1",
      flags: FLAGS,
      role: null,
      intakeCount: 0,
    });
    expect(outsider.some((entry) => entry.key === "runners")).toBe(false);
  });

  it("pins the SHELL-055 oracle matrix: guest sets and the nonzero badge", () => {
    const base = { workspaceSlug: "acme", projectId: "p1", flags: FLAGS };
    const guest = selectSubnavEntries({ ...base, role: 5, intakeCount: 1 });
    expect(guest.map((entry) => entry.name)).toEqual(["Work Items", "Views", "Pages", "Intake", "Schedulers"]);
    expect(guest.find((entry) => entry.key === "schedulers")?.href).toBe("/acme/projects/p1/schedulers/");
    expect(guest.find((entry) => entry.key === "intake")).toMatchObject({ badge: 1 });
    const member = selectSubnavEntries({ ...base, role: 15, intakeCount: 0 });
    expect(member.map((entry) => entry.key)).toContain("runners");
    expect(member.find((entry) => entry.key === "intake")?.badge).toBeNull();
  });
});

describe("resolveProjectRowHref", () => {
  const tabs = selectProjectTabs({ workspaceSlug: "acme", projectId: "p1", flags: FLAGS, role: 20 });

  it("lands on the stored default tab", () => {
    expect(resolveProjectRowHref({ tabs, defaultTab: "pages", workspaceSlug: "acme", projectId: "p1" })).toBe(
      "/acme/projects/p1/pages"
    );
  });

  it("falls back to tracking for unknown stored keys", () => {
    expect(resolveProjectRowHref({ tabs, defaultTab: "nope", workspaceSlug: "acme", projectId: "p1" })).toBe(
      "/acme/projects/p1/issues"
    );
  });

  it("falls back to tracking without available tabs", () => {
    expect(resolveProjectRowHref({ tabs: [], defaultTab: "pages", workspaceSlug: "acme", projectId: "p1" })).toBe(
      "/acme/projects/p1/issues"
    );
  });

  it("falls back to tracking when tracking itself is unavailable", () => {
    const odd = tabs.filter((tab) => tab.key !== "work_items");
    expect(odd.length).toBeGreaterThan(0);
    expect(resolveProjectRowHref({ tabs: odd, defaultTab: "nope", workspaceSlug: "acme", projectId: "p1" })).toBe(
      "/acme/projects/p1/issues"
    );
  });
});

describe("route guards", () => {
  it("treats project, drafts and views routes as group routes", () => {
    expect(isProjectsGroupRoute("/acme/projects")).toBe(true);
    expect(isProjectsGroupRoute("/acme/projects/p1/issues")).toBe(true);
    expect(isProjectsGroupRoute("/acme/drafts/")).toBe(true);
    expect(isProjectsGroupRoute("/acme/workspace-views/all-issues/")).toBe(true);
    expect(isProjectsGroupRoute("/acme/")).toBe(false);
  });

  it("lights the aggregate row on workspace views only", () => {
    expect(isAggregateRowActive("/acme/workspace-views/all-issues/", "acme")).toBe(true);
    expect(isAggregateRowActive("/acme/", "acme")).toBe(false);
  });

  it("lights sub-navigation entries at or under their address", () => {
    expect(isSubnavEntryActive("/acme/projects/p1/issues/", "/acme/projects/p1/issues/")).toBe(true);
    expect(isSubnavEntryActive("/acme/projects/p1/issues", "/acme/projects/p1/issues/")).toBe(true);
    expect(isSubnavEntryActive("/acme/projects/p1/issues/7", "/acme/projects/p1/issues/")).toBe(true);
    expect(isSubnavEntryActive("/acme/projects/p1/pages/", "/acme/projects/p1/issues/")).toBe(false);
    expect(isSubnavEntryActive("/", "/")).toBe(true);
  });
});
