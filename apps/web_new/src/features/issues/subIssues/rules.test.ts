// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sub-issue rules (parity/specs/issues.md §15 + §22, ISS-173–178/238):
// the paged envelope, the full-set rollup, filter vocabulary and
// matching, date bounds, ordering, grouping and the row reads.
import type { SubIssueItem, SubIssuesResponse } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import type { StateOption } from "../../../shared/pickers/states.js";
import {
  appliedFilterCount,
  defaultDisplayProperties,
  defaultSubIssueDisplay,
  displayChangeCount,
  EPIC_SUB_ISSUE_TITLE,
  emptySubIssueFilters,
  filterSubIssues,
  groupSubIssues,
  hasActiveFilters,
  isPropertyVisible,
  isSubIssueOverdue,
  matchesDateBounds,
  nextSubIssueCursor,
  orderSubIssues,
  pageRows,
  parseDateBound,
  setSubIssueFilter,
  showsMergedDateRange,
  subIssueRollup,
  SUB_ISSUE_FILTER_KEYS,
  SUB_ISSUE_FILTER_LABELS,
  SUB_ISSUE_PAGE_SIZE,
  SUB_ISSUE_TITLE,
  subIssuesExhausted,
  subIssueTitle,
  subIssueTotal,
  toggleDisplayProperty,
  toggleSubIssueFilter,
} from "./rules.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const ADA = "423e4567-e89b-12d3-a456-426614174003";
const GRACE = "523e4567-e89b-12d3-a456-426614174004";
const BACKLOG = "623e4567-e89b-12d3-a456-426614174005";
const WORKING = "723e4567-e89b-12d3-a456-426614174006";
const DONE = "823e4567-e89b-12d3-a456-426614174007";

function row(overrides: Partial<SubIssueItem> = {}): SubIssueItem {
  return {
    id: "923e4567-e89b-12d3-a456-426614174008",
    name: "Child",
    state_id: WORKING,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: "2026-10-01",
    target_date: "2026-10-20",
    sequence_id: 3,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    state_group: "started",
    ...overrides,
  };
}

function envelope(overrides: Partial<SubIssuesResponse> = {}): SubIssuesResponse {
  return {
    sub_issues: [],
    state_distribution: {},
    ...overrides,
  };
}

const TODAY = new Date("2026-10-18T15:00:00Z");

describe("titles and display defaults", () => {
  it("names the widget for its parent kind", () => {
    expect(subIssueTitle(false)).toBe(SUB_ISSUE_TITLE);
    expect(subIssueTitle(true)).toBe(EPIC_SUB_ISSUE_TITLE);
  });

  it("offers the seven filter dimensions in render order with labels", () => {
    expect(SUB_ISSUE_FILTER_KEYS).toEqual([
      "priority",
      "state_group",
      "state",
      "project",
      "assignees",
      "start_date",
      "target_date",
    ]);
    expect(Object.keys(SUB_ISSUE_FILTER_LABELS)).toEqual([...SUB_ISSUE_FILTER_KEYS]);
  });

  it("starts with no filters and a newest-first, ungrouped display", () => {
    const filters = emptySubIssueFilters();
    expect(hasActiveFilters(filters)).toBe(false);
    expect(appliedFilterCount(filters)).toBe(0);
    const display = defaultSubIssueDisplay();
    expect(display.groupBy).toBeNull();
    expect(display.orderBy).toBe("-created_at");
    expect(displayChangeCount(display)).toBe(0);
    expect(isPropertyVisible(display.properties, "state")).toBe(true);
  });

  it("toggles a dimension value on and off without touching the others", () => {
    const once = toggleSubIssueFilter(emptySubIssueFilters(), "priority", "high");
    expect(once.priority).toEqual(["high"]);
    expect(once.state).toEqual([]);
    const twice = toggleSubIssueFilter(once, "priority", "high");
    expect(twice.priority).toEqual([]);
    expect(once.priority).toEqual(["high"]);
  });

  it("replaces one dimension's values and counts only dimensions in use", () => {
    const filters = setSubIssueFilter(emptySubIssueFilters(), "start_date", ["2026-10-01;after"]);
    expect(appliedFilterCount(filters)).toBe(1);
    expect(hasActiveFilters(filters)).toBe(true);
    expect(filters.start_date).toEqual(["2026-10-01;after"]);
    expect(filters.priority).toEqual([]);
  });

  it("reports display changes against the defaults and toggles a column back", () => {
    const hidden = toggleDisplayProperty(defaultDisplayProperties(), "priority");
    expect(hidden.priority).toBe(false);
    expect(displayChangeCount({ groupBy: "state", orderBy: "-priority", properties: hidden })).toBe(3);
    expect(displayChangeCount({ ...defaultSubIssueDisplay(), groupBy: "state", orderBy: "-priority" })).toBe(2);
    expect(displayChangeCount({ ...defaultSubIssueDisplay(), orderBy: "start_date" })).toBe(1);
    expect(isPropertyVisible(hidden, "priority")).toBe(false);
  });
});

describe("paged envelope", () => {
  it("reads rows and the full-set total from the envelope", () => {
    const page = envelope({ sub_issues: [row(), row({ id: ADA })], total_count: 12 });
    expect(pageRows(page).map((entry) => entry.id)).toEqual([row().id, ADA]);
    expect(subIssueTotal(page)).toBe(12);
  });

  it("falls back to the distribution sum, then the page length", () => {
    expect(subIssueTotal(envelope({ state_distribution: { started: [ADA, GRACE], completed: [BACKLOG] } }))).toBe(3);
    expect(subIssueTotal(envelope({ sub_issues: [row()] }))).toBe(1);
  });

  it("rolls up completed children over the full-set total", () => {
    const page = envelope({ total_count: 4, state_distribution: { completed: [ADA, GRACE] } });
    expect(subIssueRollup(page)).toEqual({ done: 2, total: 4, percent: 50 });
  });

  it("reads zero progress for a parent with no children", () => {
    expect(subIssueRollup(envelope())).toEqual({ done: 0, total: 0, percent: 0 });
  });

  it("caps the ring at one hundred percent", () => {
    expect(subIssueRollup(envelope({ total_count: 1, state_distribution: { completed: [ADA, GRACE] } })).percent).toBe(
      100
    );
  });

  it("takes a next cursor only when the server reports more pages", () => {
    expect(nextSubIssueCursor(envelope({ next_cursor: "c1" }))).toBe("c1");
    expect(nextSubIssueCursor(envelope({ next_cursor: "" }))).toBeNull();
    expect(nextSubIssueCursor(envelope({ next_cursor: "c1", next_page_results: false }))).toBeNull();
    expect(nextSubIssueCursor(envelope())).toBeNull();
  });

  it("closes the set on a short page even when a cursor is present", () => {
    const short = envelope({ sub_issues: [row()], next_cursor: "c1" });
    expect(subIssuesExhausted(short)).toBe(true);
    const full = envelope({ sub_issues: Array.from({ length: SUB_ISSUE_PAGE_SIZE }, () => row()), next_cursor: "c1" });
    expect(subIssuesExhausted(full)).toBe(false);
    expect(subIssuesExhausted(short, 1)).toBe(false);
  });
});

describe("date bounds", () => {
  it("resolves relative windows in days, weeks and months", () => {
    expect(parseDateBound("3_days;after;fromnow", TODAY)).toEqual({ type: "after", day: "2026-10-21" });
    expect(parseDateBound("2_weeks;before;fromnow", TODAY)).toEqual({ type: "before", day: "2026-10-04" });
    expect(parseDateBound("1_months;after;fromnow", TODAY)).toEqual({ type: "after", day: "2026-11-17" });
    expect(parseDateBound("1_months;before;fromnow", TODAY)).toEqual({ type: "before", day: "2026-09-18" });
  });

  it("resolves an absolute day with its direction", () => {
    expect(parseDateBound("2026-10-01;after", TODAY)).toEqual({ type: "after", day: "2026-10-01" });
    expect(parseDateBound(" 2026-10-01;before ", TODAY)).toEqual({ type: "before", day: "2026-10-01" });
  });

  it("reads a bare day as exactly that day", () => {
    expect(parseDateBound("2026-10-05", TODAY)).toEqual({ type: "on", day: "2026-10-05" });
    expect(parseDateBound(" 2026-10-05 ", TODAY)).toEqual({ type: "on", day: "2026-10-05" });
  });

  it("ignores strings it cannot read", () => {
    expect(parseDateBound("tomorrow", TODAY)).toBeNull();
    expect(parseDateBound("2026-10-01;during", TODAY)).toBeNull();
    expect(parseDateBound("2026-1-1;after", TODAY)).toBeNull();
    expect(parseDateBound("2026-10-01;", TODAY)).toBeNull();
  });

  it("matches inclusive bounds and skips unreadable entries", () => {
    expect(matchesDateBounds(null, [], TODAY)).toBe(true);
    expect(matchesDateBounds(null, ["2026-10-01;after"], TODAY)).toBe(false);
    expect(matchesDateBounds(undefined, ["nonsense"], TODAY)).toBe(true);
    expect(matchesDateBounds("2026-10-01", ["2026-10-01;after"], TODAY)).toBe(true);
    expect(matchesDateBounds("2026-09-30", ["2026-10-01;after"], TODAY)).toBe(false);
    expect(matchesDateBounds("2026-10-18T00:00:00Z", ["2026-10-18;before"], TODAY)).toBe(true);
    expect(matchesDateBounds("2026-10-18", ["2026-10-01;after", "2026-10-31;before"], TODAY)).toBe(true);
  });

  it("keeps only the exact day for a stored On value", () => {
    expect(matchesDateBounds("2026-10-05", ["2026-10-05"], TODAY)).toBe(true);
    expect(matchesDateBounds("2026-10-06", ["2026-10-05"], TODAY)).toBe(false);
    expect(matchesDateBounds("2026-10-04", ["2026-10-05"], TODAY)).toBe(false);
    expect(matchesDateBounds("2026-10-05T09:30:00Z", ["2026-10-05"], TODAY)).toBe(true);
    expect(matchesDateBounds(null, ["2026-10-05"], TODAY)).toBe(false);
  });

  it("filters a row by the On value the menu stores", () => {
    const filters = { ...emptySubIssueFilters(), start_date: ["2026-10-05"] };
    const rows = [
      row({ id: ADA, start_date: "2026-10-04" }),
      row({ id: GRACE, start_date: "2026-10-05" }),
      row({ id: BACKLOG, start_date: "2026-10-06" }),
    ];
    expect(filterSubIssues(rows, filters, TODAY).map((entry) => entry.id)).toEqual([GRACE]);
  });

  it("reads After as a lower bound and Before as an upper bound on either date", () => {
    for (const key of ["start_date", "target_date"] as const) {
      const after = { ...emptySubIssueFilters(), [key]: ["2026-10-10;after"] };
      expect(filterSubIssues([row({ [key]: "2026-10-09" })], after, TODAY)).toEqual([]);
      expect(filterSubIssues([row({ [key]: "2026-10-10" })], after, TODAY)).toHaveLength(1);
      expect(filterSubIssues([row({ [key]: "2026-10-11" })], after, TODAY)).toHaveLength(1);
      const before = { ...emptySubIssueFilters(), [key]: ["2026-10-10;before"] };
      expect(filterSubIssues([row({ [key]: "2026-10-09" })], before, TODAY)).toHaveLength(1);
      expect(filterSubIssues([row({ [key]: "2026-10-10" })], before, TODAY)).toHaveLength(1);
      expect(filterSubIssues([row({ [key]: "2026-10-11" })], before, TODAY)).toEqual([]);
    }
  });
});

describe("filtering one page", () => {
  it("keeps every row when nothing is applied", () => {
    const rows = [row(), row({ id: ADA, priority: "urgent" })];
    expect(filterSubIssues(rows, emptySubIssueFilters(), TODAY)).toHaveLength(2);
  });

  it("ANDs dimensions and ORs values inside one", () => {
    const rows = [
      row({ id: ADA, priority: "high", state_group: "started" }),
      row({ id: GRACE, priority: "urgent", state_group: "started" }),
      row({ id: BACKLOG, priority: "high", state_group: "backlog" }),
    ];
    const filters = {
      ...emptySubIssueFilters(),
      priority: ["high", "urgent"],
      state_group: ["started"],
    };
    expect(filterSubIssues(rows, filters, TODAY).map((entry) => entry.id)).toEqual([ADA, GRACE]);
  });

  it("drops a row with no value on an active dimension", () => {
    const filters = { ...emptySubIssueFilters(), priority: ["high"] };
    expect(filterSubIssues([row({ priority: "none" })], filters, TODAY)).toEqual([]);
    const state = { ...emptySubIssueFilters(), state_group: ["started"] };
    expect(filterSubIssues([row({ state_group: null })], state, TODAY)).toEqual([]);
  });

  it("matches state, project and any assignee", () => {
    const rows = [
      row({ id: ADA, state_id: WORKING, project_id: PROJECT, assignee_ids: [ADA] }),
      row({ id: GRACE, state_id: BACKLOG, project_id: OTHER_PROJECT, assignee_ids: [GRACE] }),
    ];
    const byState = filterSubIssues(rows, { ...emptySubIssueFilters(), state: [WORKING] }, TODAY);
    expect(byState.map((entry) => entry.id)).toEqual([ADA]);
    const byProject = filterSubIssues(rows, { ...emptySubIssueFilters(), project: [OTHER_PROJECT] }, TODAY);
    expect(byProject.map((entry) => entry.id)).toEqual([GRACE]);
    const byAssignee = filterSubIssues(rows, { ...emptySubIssueFilters(), assignees: [GRACE, "nobody"] }, TODAY);
    expect(byAssignee.map((entry) => entry.id)).toEqual([GRACE]);
  });

  it("applies both date dimensions", () => {
    const rows = [
      row({ id: ADA, start_date: "2026-10-01", target_date: "2026-10-10" }),
      row({ id: GRACE, start_date: "2026-11-01", target_date: "2026-11-10" }),
      row({ id: BACKLOG, start_date: null, target_date: null }),
    ];
    const filters = {
      ...emptySubIssueFilters(),
      start_date: ["2026-10-15;before"],
      target_date: ["2026-10-05;after"],
    };
    expect(filterSubIssues(rows, filters, TODAY).map((entry) => entry.id)).toEqual([ADA]);
  });
});

describe("ordering", () => {
  const rows = [
    row({
      id: ADA,
      created_at: "2026-10-01T00:00:00Z",
      updated_at: "2026-10-02T00:00:00Z",
      priority: "low",
      start_date: "2026-10-05",
    }),
    row({
      id: GRACE,
      created_at: "2026-10-03T00:00:00Z",
      updated_at: "2026-10-01T00:00:00Z",
      priority: "urgent",
      start_date: null,
    }),
    row({
      id: BACKLOG,
      created_at: "2026-10-02T00:00:00Z",
      updated_at: "2026-10-03T00:00:00Z",
      priority: "high",
      start_date: "2026-10-01",
    }),
  ];

  it("orders newest first by default", () => {
    expect(orderSubIssues(rows, "-created_at").map((entry) => entry.id)).toEqual([GRACE, BACKLOG, ADA]);
  });

  it("orders by the latest update", () => {
    expect(orderSubIssues(rows, "-updated_at").map((entry) => entry.id)).toEqual([BACKLOG, ADA, GRACE]);
  });

  it("orders dated rows first, earliest first", () => {
    expect(orderSubIssues(rows, "start_date").map((entry) => entry.id)).toEqual([BACKLOG, ADA, GRACE]);
  });

  it("orders urgent through none with unknown levels last", () => {
    const withNone = [...rows, row({ id: WORKING, priority: "none" })];
    const ordered = orderSubIssues(withNone, "-priority").map((entry) => entry.priority);
    expect(ordered).toEqual(["urgent", "high", "low", "none"]);
  });

  it("falls back to the newest-first order for an unknown key", () => {
    expect(orderSubIssues(rows, "bogus" as never).map((entry) => entry.id)).toEqual([GRACE, BACKLOG, ADA]);
    expect(
      orderSubIssues(
        [row({ id: ADA, created_at: "2026-01-01T00:00:00Z" }), row({ id: GRACE, created_at: "2026-01-01T00:00:00Z" })],
        "-created_at"
      )
    ).toHaveLength(2);
    expect(
      orderSubIssues(
        [row({ id: ADA, updated_at: "2026-01-01T00:00:00Z" }), row({ id: GRACE, updated_at: "2026-01-01T00:00:00Z" })],
        "-updated_at"
      )
    ).toHaveLength(2);
    expect(
      orderSubIssues(
        [row({ id: ADA, start_date: "2026-01-01" }), row({ id: GRACE, start_date: "2026-01-01" })],
        "start_date"
      )
    ).toHaveLength(2);
  });
});

describe("grouping", () => {
  const states: StateOption[] = [
    { id: BACKLOG, name: "Backlog", group: "backlog", color: "#000" },
    { id: WORKING, name: "In progress", group: "started", color: "#111" },
    { id: DONE, name: "Done", group: "completed", color: "#222" },
  ];
  const rows = [
    row({ id: "123e4567-e89b-12d3-a456-426614174001", state_id: WORKING, priority: "high", assignee_ids: [ADA] }),
    row({ id: "123e4567-e89b-12d3-a456-426614174002", state_id: BACKLOG, priority: "urgent", assignee_ids: [] }),
    row({ id: "123e4567-e89b-12d3-a456-426614174003", state_id: DONE, priority: "none", assignee_ids: [GRACE] }),
    row({
      id: "123e4567-e89b-12d3-a456-426614174004",
      state_id: "unknown",
      priority: "low",
      assignee_ids: [ADA, GRACE],
    }),
  ];

  it("returns one unnamed family when ungrouped", () => {
    const groups = groupSubIssues(rows, null, "-created_at");
    expect(groups).toHaveLength(1);
    expect(groups[0]?.id).toBe("");
    expect(groups[0]?.rows).toHaveLength(4);
  });

  it("orders groups by the workflow and puts unknown ids in None last", () => {
    const groups = groupSubIssues(rows, "state", "-created_at", { states });
    expect(groups.map((group) => group.label)).toEqual(["Backlog", "In progress", "Done", "None"]);
  });

  it("groups priorities down the ladder and merges rows without states", () => {
    const groups = groupSubIssues(rows, "priority", "-created_at");
    expect(groups.map((group) => group.id)).toEqual(["urgent", "high", "low", "none"]);
    expect(groups.map((group) => group.label)).toEqual(["Urgent", "High", "Low", "None"]);
    const statesOnly = groupSubIssues(rows, "state", "-created_at");
    expect(statesOnly.map((group) => group.label)).toEqual(["None"]);
  });

  it("groups assignees by the first member in picker order, None last", () => {
    const groups = groupSubIssues(rows, "assignees", "-created_at", {
      memberIds: [ADA, GRACE],
      memberNames: { [ADA]: "Ada", [GRACE]: "Grace" },
    });
    expect(groups.map((group) => group.label)).toEqual(["Ada", "Grace", "None"]);
    expect(groups[0]?.rows).toHaveLength(2);
    const byRow = groupSubIssues(rows, "assignees", "-created_at");
    expect(byRow.map((group) => group.label)).toContain(ADA);
  });
});

describe("row reads", () => {
  it("marks a due day on or before today as overdue unless closed", () => {
    expect(isSubIssueOverdue(row({ target_date: "2026-10-18" }), TODAY)).toBe(true);
    expect(isSubIssueOverdue(row({ target_date: "2026-10-17", state_group: "review" }), TODAY)).toBe(true);
    expect(isSubIssueOverdue(row({ target_date: "2026-10-19" }), TODAY)).toBe(false);
    expect(isSubIssueOverdue(row({ target_date: null }), TODAY)).toBe(false);
    expect(isSubIssueOverdue(row({ target_date: undefined }), TODAY)).toBe(false);
    expect(isSubIssueOverdue(row({ target_date: "2026-10-01", state_group: "completed" }), TODAY)).toBe(false);
    expect(isSubIssueOverdue(row({ target_date: "2026-10-01", state_group: "cancelled" }), TODAY)).toBe(false);
    expect(isSubIssueOverdue(row({ target_date: "2026-10-01", state_group: null }), TODAY)).toBe(true);
  });

  it("merges the range only when both ends and both columns are on", () => {
    const both = defaultDisplayProperties();
    expect(showsMergedDateRange(row(), both)).toBe(true);
    expect(showsMergedDateRange(row({ start_date: null }), both)).toBe(false);
    expect(showsMergedDateRange(row({ target_date: undefined }), both)).toBe(false);
    expect(showsMergedDateRange(row(), { ...both, start_date: false })).toBe(false);
    expect(showsMergedDateRange(row(), { ...both, due_date: false })).toBe(false);
  });
});

describe("branch edges", () => {
  it("closes a page with no cursor and keeps a full page open without one", () => {
    const noCursor = envelope({ sub_issues: [row()] });
    expect(nextSubIssueCursor(noCursor)).toBeNull();
    expect(subIssuesExhausted(noCursor)).toBe(true);
  });

  it("sorts rows with no timestamps together and ties in place", () => {
    const blank = [row({ id: ADA, created_at: undefined }), row({ id: GRACE, created_at: undefined })];
    const ordered = orderSubIssues(blank, "-updated_at");
    expect(ordered.map((entry) => entry.id)).toEqual([ADA, GRACE]);
    expect(orderSubIssues(blank, "-created_at").map((entry) => entry.id)).toEqual([ADA, GRACE]);
  });

  it("keeps one equal-start-date pair in input order", () => {
    const sameDay = [row({ id: ADA }), row({ id: GRACE })];
    expect(orderSubIssues(sameDay, "start_date").map((entry) => entry.id)).toEqual([ADA, GRACE]);
    expect(
      orderSubIssues(sameDay, "start_date")
        .reverse()
        .map((entry) => entry.id)
    ).toEqual([GRACE, ADA]);
  });

  it("ranks a priority outside the ladder last", () => {
    const rows = [
      row({ id: ADA, priority: "urgent" }),
      row({ id: GRACE, priority: "bogus" as never }),
      row({ id: BACKLOG, priority: "bogus" as never }),
    ];
    expect(orderSubIssues(rows, "-priority").map((entry) => entry.id)).toEqual([ADA, GRACE, BACKLOG]);
  });
});
