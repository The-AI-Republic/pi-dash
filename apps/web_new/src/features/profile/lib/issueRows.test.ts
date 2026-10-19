// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Issue-tab row shaping (parity/specs/profile.md §2, PROF-006).
import { describe, expect, it } from "vitest";

import type { IssueListItem } from "@pidash/api-client";

import {
  PRIORITY_ORDER,
  PROFILE_GROUP_LABELS,
  UNGROUPED_TITLE,
  groupIssueRows,
  issueAssigneeIds,
  issueStateGroup,
  priorityLabel,
} from "./issueRows.js";
import type { ProfileStateGroup } from "./stats.js";

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
    expect(issueStateGroup(issue({ state_id: "s-1" }), groups)).toBe("started");
    expect(issueStateGroup(issue({ state_id: "s-9" }), groups)).toBe("backlog");
    expect(issueStateGroup(issue({ state_id: null }), groups)).toBe("backlog");
    expect(issueAssigneeIds(issue({ assignee_ids: ["u-1"] }))).toEqual(["u-1"]);
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
    const groups = groupIssueRows(rows, "state", stateGroups);
    expect(groups.map((group) => group.title)).toEqual(["Backlog", "Started", "Review"]);
  });

  it("groups assignees with unassigned first and an unassigned title", () => {
    const rows = [
      issue({ id: "i-b", assignee_ids: ["u-b"] }),
      issue({ id: "i-a", assignee_ids: ["u-a"] }),
      issue({ id: "i-none", assignee_ids: [] }),
    ];
    const groups = groupIssueRows(rows, "assignees", new Map(), new Map([["u-b", "Bea"]]));
    expect(groups.map((group) => group.title)).toEqual(["Unassigned", "u-a", "Bea"]);
    expect(groups[0]?.rows.map((row) => row.id)).toEqual(["i-none"]);
  });

  it("puts a row with several assignees into each group once", () => {
    const groups = groupIssueRows([issue({ id: "i-1", assignee_ids: ["u-a", "u-b"] })], "assignees");
    expect(groups.map((group) => group.id)).toEqual(["u-a", "u-b"]);
  });
});
