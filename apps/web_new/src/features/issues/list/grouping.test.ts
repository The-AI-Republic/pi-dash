// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { GroupedIssueRow, IssueListItem } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  FLAT_GROUP_TITLE,
  creationDisabledForGroup,
  defaultGroupTitle,
  flatSection,
  groupTitle,
  groupedSections,
  mergeGroupedPage,
  normalizeGroupedRow,
  orderGroupKeys,
  sectionHasMore,
  sectionNextCursor,
  sectionPrefill,
  serverGroupBy,
} from "./grouping.js";

function row(id: string, sequence = 1): IssueListItem {
  return {
    id,
    name: `Issue ${sequence}`,
    state_id: null,
    sort_order: 1,
    completed_at: null,
    priority: "none",
    sequence_id: sequence,
    project_id: "project",
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: 0,
    attachment_count: 0,
    link_count: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

describe("serverGroupBy", () => {
  it.each([
    ["state", "state_id"],
    ["priority", "priority"],
    ["labels", "labels__id"],
    ["assignees", "assignees__id"],
    ["cycle", "cycle_id"],
    ["module", "issue_module__module_id"],
    ["project", "project_id"],
    ["created_by", "created_by"],
    ["state_detail.group", "state__group"],
  ])("maps %s to %s", (dimension, expected) => {
    expect(serverGroupBy(dimension)).toBe(expected);
  });

  it("returns null for flat and unknown dimensions", () => {
    expect(serverGroupBy(null)).toBeNull();
    expect(serverGroupBy("team_project")).toBeNull();
  });
});

describe("group titles and ordering", () => {
  it("uses the flat title the oracle names", () => {
    expect(flatSection([], 0).title).toBe(FLAT_GROUP_TITLE);
  });

  it.each([
    ["urgent", "Urgent"],
    ["high", "High"],
    ["medium", "Medium"],
    ["low", "Low"],
    ["none", "None"],
  ])("titles priority %s", (key, title) => {
    expect(defaultGroupTitle("priority", key)).toBe(title);
  });

  it.each([
    ["backlog", "Backlog"],
    ["unstarted", "Unstarted"],
    ["started", "Started"],
    ["review", "Review"],
    ["test", "Test"],
    ["completed", "Completed"],
    ["cancelled", "Canceled"],
  ])("titles state group %s", (key, title) => {
    expect(defaultGroupTitle("state_detail.group", key)).toBe(title);
  });

  it("prefers resolved names, then fixed titles, then the raw key", () => {
    const names = new Map([["id-1", "Doing"]]);
    expect(groupTitle("id-1", names)).toBe("Doing");
    expect(groupTitle("None", names)).toBe("None");
    expect(groupTitle("high", names, { dimension: "priority" })).toBe("High");
    expect(groupTitle("id-2", names, { dimension: "labels" })).toBe("id-2");
    expect(groupTitle("id-3", names, { titleFallback: "Fallback" })).toBe("Fallback");
  });

  it("keeps named keys in order with None last", () => {
    expect(orderGroupKeys(["None", "b", "a", "None"])).toEqual(["b", "a", "None", "None"]);
  });

  it("disables creation only for creator grouping", () => {
    expect(creationDisabledForGroup("created_by")).toBe(true);
    expect(creationDisabledForGroup("state")).toBe(false);
    expect(creationDisabledForGroup(null)).toBe(false);
  });
});

describe("grouped sections", () => {
  it("orders buckets, filters empties and titles them", () => {
    const sections = groupedSections(
      {
        results: {
          None: { results: [row("none")], total_results: 1 },
          high: { results: [row("high")], total_results: 2 },
          low: { results: [row("low")], total_results: 0 },
        },
      },
      new Map(),
      { showEmptyGroups: false, dimension: "priority" }
    );
    expect(sections.map((section) => [section.key, section.title, section.value])).toEqual([
      ["high", "High", "high"],
      ["None", "None", null],
    ]);
    expect(sections[0]?.total).toBe(2);
  });

  it("keeps empty buckets when the display flag is on", () => {
    const sections = groupedSections({ results: { todo: { results: [], total_results: 0 } } }, new Map(), {
      showEmptyGroups: true,
      dimension: "state",
    });
    expect(sections.map((section) => section.key)).toEqual(["todo"]);
  });

  it("folds m2m join keys back into arrays and defaults absent arrays", () => {
    const grouped: GroupedIssueRow = {
      ...row("one"),
      label_ids: undefined,
      assignee_ids: undefined,
      module_ids: undefined,
      labels__id: "label-1",
      assignees__id: "member-1",
      issue_module__module_id: "module-1",
    };
    expect(normalizeGroupedRow(grouped, "labels")).toMatchObject({
      label_ids: ["label-1"],
      assignee_ids: ["member-1"],
      module_ids: ["module-1"],
    });
    expect(normalizeGroupedRow({ ...grouped, labels__id: null }, "labels").label_ids).toEqual([]);
    expect(normalizeGroupedRow({ ...grouped, labels__id: undefined, assignee_ids: ["kept"] }, "labels")).toMatchObject({
      label_ids: [],
      assignee_ids: ["kept"],
    });
  });

  it("merges pages without duplicating rows and keeps totals fresh", () => {
    const current = [
      { key: "a", title: "A", rows: [row("1"), row("2")], total: 4, value: "a" },
      { key: "None", title: "None", rows: [], total: 0, value: null },
    ];
    const incoming = [
      { key: "a", title: "A", rows: [row("2"), row("3")], total: 5, value: "a" },
      { key: "b", title: "B", rows: [row("4")], total: 1, value: "b" },
    ];
    const merged = mergeGroupedPage(current, incoming);
    expect(merged.map((section) => section.key)).toEqual(["a", "b", "None"]);
    expect(merged[0]?.rows.map((entry) => entry.id)).toEqual(["1", "2", "3"]);
    expect(merged[0]?.total).toBe(5);
  });

  it("reports paging state and its next cursor", () => {
    const section = { key: "a", title: "A", rows: [row("1")], total: 51, value: "a" };
    expect(sectionHasMore(section)).toBe(true);
    expect(sectionNextCursor(section, 50)).toBe("50:0:0");
    const done = { ...section, total: 1 };
    expect(sectionHasMore(done)).toBe(false);
    expect(sectionNextCursor(done, 50)).toBeNull();
    expect(sectionNextCursor({ ...section, rows: [] }, 50)).toBe("50:0:0");
  });
});

describe("sectionPrefill", () => {
  const section = { key: "value", title: "Value", rows: [], total: 0, value: "value" };

  it.each([
    ["state", { state_id: "value" }],
    ["priority", { priority: "value" }],
    ["labels", { label_ids: ["value"] }],
    ["assignees", { assignee_ids: ["value"] }],
    ["cycle", { cycle_id: "value" }],
    ["module", { module_ids: ["value"] }],
    ["project", {}],
    ["created_by", {}],
  ])("maps %s grouping to its create field", (dimension, expected) => {
    expect(sectionPrefill(dimension, section)).toEqual(expected);
  });

  it("returns nothing for flat and None buckets", () => {
    expect(sectionPrefill(null, section)).toEqual({});
    expect(sectionPrefill("state", { ...section, value: null })).toEqual({});
  });
});
