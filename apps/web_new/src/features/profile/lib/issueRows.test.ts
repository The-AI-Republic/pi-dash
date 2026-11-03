// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-tab row shaping (parity/specs/profile.md §2, PROF-006).
import { describe, expect, it } from "vitest";

import type { IssueListItem } from "@pidash/api-client";

import {
  EMPTY_STATE_LOOKUP,
  PRIORITY_ORDER,
  PROFILE_GROUP_LABELS,
  STATE_GROUP_TITLES,
  UNGROUPED_TITLE,
  groupIssueRows,
  issueAssigneeIds,
  issueStateGroup,
  stateGroupFor,
  isProfileStateGroup,
  priorityLabel,
  visibleIssueRows,
  type ProfileStateLookup,
} from "./issueRows.js";
import type { ProfileStateGroup } from "./stats.js";

function lookup(groups: ReadonlyMap<string, ProfileStateGroup>): ProfileStateLookup {
  return { group: (id) => groups.get(id), name: () => undefined };
}

function issue(overrides: Partial<IssueListItem> = {}): IssueListItem {
  return {
    id: "i-1",
    name: "Fix the thing",
    state_id: null,
    sort_order: 0,
    completed_at: null,
    priority: "none",
    sequence_id: 1,
    project_id: "p-1",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: 0,
    link_count: 0,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    ...overrides,
  } as IssueListItem;
}

describe("issue row vocabulary", () => {
  it("capitalises priority labels and pins the group labels", () => {
    expect(priorityLabel("urgent")).toBe("Urgent");
    expect(priorityLabel("")).toBe("");
    expect(PRIORITY_ORDER).toEqual(["urgent", "high", "medium", "low", "none"]);
    expect(PROFILE_GROUP_LABELS).toEqual({ priority: "Priority", state: "States", assignees: "Assignees" });
    expect(UNGROUPED_TITLE).toBe("All work items");
  });

  it("reads an issue's state group, defaulting to backlog", () => {
    const groups = new Map<string, ProfileStateGroup>([["s-1", "started"]]);
    expect(issueStateGroup(issue({ state_id: "s-1" }), lookup(groups))).toBe("started");
    expect(issueStateGroup(issue({ state_id: "s-9" }), lookup(groups))).toBe("backlog");
    expect(issueStateGroup(issue({ state_id: null }), lookup(groups))).toBe("backlog");
    expect(issueAssigneeIds(issue({ assignee_ids: ["u-1"] }))).toEqual(["u-1"]);
  });

  it("recognises the seven profile state groups and nothing else", () => {
    expect(isProfileStateGroup("started")).toBe(true);
    expect(isProfileStateGroup("cancelled")).toBe(true);
    expect(isProfileStateGroup("in_progress")).toBe(false);
    expect(Object.values(STATE_GROUP_TITLES)).toEqual([
      "Backlog",
      "Unstarted",
      "Started",
      "Review",
      "Test",
      "Completed",
      "Canceled",
    ]);
  });
});

describe("groupIssueRows", () => {
  it("answers an empty array for an empty page and one section when ungrouped", () => {
    expect(groupIssueRows([], null)).toEqual([]);
    const grouped = groupIssueRows([issue(), issue({ id: "i-2" })], null);
    expect(grouped).toHaveLength(1);
    expect(grouped[0]?.title).toBe(UNGROUPED_TITLE);
    expect(grouped[0]?.rows.map((row) => row.id)).toEqual(["i-1", "i-2"]);
  });

  it("orders priority groups urgent first and titles them capitalised", () => {
    const rows = [
      issue({ id: "i-low", priority: "low" }),
      issue({ id: "i-urgent", priority: "urgent" }),
      issue({ id: "i-none", priority: "none" }),
    ];
    const groups = groupIssueRows(rows, "priority");
    expect(groups.map((group) => group.title)).toEqual(["Urgent", "Low", "None"]);
    expect(groups[0]?.rows.map((row) => row.id)).toEqual(["i-urgent"]);
  });

  it("orders state groups backlog first and titles them by group", () => {
    const stateGroups = new Map<string, ProfileStateGroup>([
      ["s-review", "review"],
      ["s-start", "started"],
    ]);
    const rows = [
      issue({ id: "i-review", state_id: "s-review" }),
      issue({ id: "i-start", state_id: "s-start" }),
      issue({ id: "i-backlog", state_id: null }),
    ];
    const groups = groupIssueRows(rows, "state", lookup(stateGroups));
    expect(groups.map((group) => group.title)).toEqual(["Backlog", "Started", "Review"]);
  });

  it("zero-fills the seven state groups in the server's order when asked", () => {
    const groups = groupIssueRows(
      [issue({ id: "i-1", state_id: "s-review" })],
      "state",
      lookup(new Map([["s-review", "review"]])),
      { showEmptyGroups: true }
    );
    expect(groups.map((group) => group.id)).toEqual([
      "backlog",
      "unstarted",
      "started",
      "review",
      "test",
      "completed",
      "cancelled",
    ]);
    expect(groups.map((group) => group.rows.length)).toEqual([0, 0, 0, 1, 0, 0, 0]);
    expect(groupIssueRows([], "state", EMPTY_STATE_LOOKUP, { showEmptyGroups: true })).toHaveLength(7);
  });

  it("reads an unknown state id as a state group key and ranks it last", () => {
    const groups = groupIssueRows(
      [issue({ id: "i-unknown", state_id: "s-unknown" }), issue({ id: "i-done", state_id: "s-done" })],
      "state",
      { group: (id) => (id === "s-done" ? "completed" : undefined), name: () => undefined }
    );
    expect(groups.map((group) => group.title)).toEqual(["Backlog", "Completed"]);
  });

  it("resolves a grouping key to its seven-group bucket", () => {
    // A project may answer a group outside the seven; that must not leak
    // out of the lookup, so the cast feeds one through on purpose.
    const named = {
      group: (id: string) => (id === "s-done" ? "completed" : id === "s-odd" ? "sideways" : undefined),
      name: () => undefined,
    } as unknown as ProfileStateLookup;
    expect(stateGroupFor("started", EMPTY_STATE_LOOKUP)).toBe("started");
    expect(stateGroupFor("s-done", named)).toBe("completed");
    expect(stateGroupFor("s-odd", named)).toBe("backlog");
    expect(stateGroupFor("s-missing", named)).toBe("backlog");
    expect(stateGroupFor("s-missing", EMPTY_STATE_LOOKUP)).toBe("backlog");
  });

  it("groups assignees with unassigned first and an unassigned title", () => {
    const rows = [
      issue({ id: "i-b", assignee_ids: ["u-b"] }),
      issue({ id: "i-a", assignee_ids: ["u-a"] }),
      issue({ id: "i-none", assignee_ids: [] }),
    ];
    const groups = groupIssueRows(rows, "assignees", EMPTY_STATE_LOOKUP, { memberNames: new Map([["u-b", "Bea"]]) });
    expect(groups.map((group) => group.title)).toEqual(["Unassigned", "u-a", "Bea"]);
    expect(groups[0]?.rows.map((row) => row.id)).toEqual(["i-none"]);
  });

  it("puts a row with several assignees into each group once", () => {
    const groups = groupIssueRows([issue({ id: "i-1", assignee_ids: ["u-a", "u-b"] })], "assignees");
    expect(groups.map((group) => group.id)).toEqual(["u-a", "u-b"]);
  });

  it("drops rows that hang under another work item unless the switch is on", () => {
    const parent = issue({ id: "i-parent" });
    const child = issue({ id: "i-child", parent_id: "i-parent" });
    expect(visibleIssueRows([parent, child], false)).toEqual([parent]);
    expect(visibleIssueRows([parent, child], true)).toEqual([parent, child]);
    expect(visibleIssueRows([], false)).toEqual([]);
    expect(visibleIssueRows([], true)).toEqual([]);
  });

  it("appends a second row to the bucket an earlier row opened", () => {
    const rows = [issue({ id: "i-1", assignee_ids: ["u-a"] }), issue({ id: "i-2", assignee_ids: ["u-a"] })];
    const groups = groupIssueRows(rows, "assignees");
    expect(groups).toHaveLength(1);
    expect(groups[0]?.rows.map((row) => row.id)).toEqual(["i-1", "i-2"]);
  });

  it("keeps unassigned ahead of a named assignee", () => {
    const rows = [issue({ id: "i-b", assignee_ids: ["u-b"] }), issue({ id: "i-none", assignee_ids: [] })];
    const groups = groupIssueRows(rows, "assignees");
    expect(groups.map((group) => group.title)).toEqual(["Unassigned", "u-b"]);
    expect(groupIssueRows([...rows].reverse(), "assignees").map((group) => group.title)).toEqual(["Unassigned", "u-b"]);
  });

  it("orders two named assignees alphabetically", () => {
    const rows = [issue({ id: "i-b", assignee_ids: ["u-b"] }), issue({ id: "i-a", assignee_ids: ["u-a"] })];
    expect(groupIssueRows(rows, "assignees").map((group) => group.title)).toEqual(["u-a", "u-b"]);
  });
});
