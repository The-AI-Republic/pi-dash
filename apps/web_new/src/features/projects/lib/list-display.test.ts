// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { ProjectDetails, WorkspaceMember } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ACCESS_OPTIONS,
  countActiveListFilters,
  CREATED_PRESETS,
  DEFAULT_LIST_SORT,
  EMPTY_LIST_FILTERS,
  filterListRows,
  formatCreatedChip,
  freshListFilters,
  hasNarrowingListFilters,
  isCustomCreatedValue,
  LIST_SORT_OPTIONS,
  matchesCreatedFilter,
  matchesListFilters,
  matchesListSearch,
  selectMemberOptions,
  sortDirectionLocked,
  sortKeyLabel,
  sortListRows,
  type ListFilters,
  type ListSort,
} from "./list-display.js";

const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function project(name: string, overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: "223e4567-e89b-12d3-a456-426614174001",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WS_ID,
    name,
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "managed_runner_disabled" },
    ],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function member(id: string, displayName: string): WorkspaceMember {
  return {
    id: `m-${id}`,
    member: {
      id,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: `https://cdn.test/${id}.png`,
      display_name: displayName,
      is_bot: false,
    },
    role: 10,
  };
}

function filters(overrides: Partial<ListFilters> = {}): ListFilters {
  return { ...freshListFilters(), ...overrides };
}

describe("sortKeyLabel", () => {
  it("names every key the menu offers", () => {
    expect(LIST_SORT_OPTIONS.map((option) => option.label)).toEqual([
      "Manual",
      "Name",
      "Created date",
      "Number of members",
    ]);
    expect(sortKeyLabel("manual")).toBe("Manual");
    expect(sortKeyLabel("name")).toBe("Name");
    expect(sortKeyLabel("created")).toBe("Created date");
    expect(sortKeyLabel("members")).toBe("Number of members");
  });

  it("falls back to the default label for unknown keys", () => {
    expect(sortKeyLabel("archived" as never)).toBe("Created date");
  });
});

describe("sortDirectionLocked", () => {
  it("locks direction for manual order only", () => {
    expect(DEFAULT_LIST_SORT).toEqual({ key: "created", direction: "ascending" });
    expect(sortDirectionLocked({ key: "manual", direction: "ascending" })).toBe(true);
    expect(sortDirectionLocked({ key: "name", direction: "ascending" })).toBe(false);
  });
});

describe("sortListRows", () => {
  const names = (rows: ProjectDetails[]): string[] => rows.map((row) => row.name);

  it("orders manually with unordered rows sinking last", () => {
    const rows = [
      project("Unordered", { sort_order: null }),
      project("Second", { sort_order: 200 }),
      project("First", { sort_order: 100 }),
      project("Also unordered", { sort_order: null }),
    ];
    const sort: ListSort = { key: "manual", direction: "ascending" };
    expect(names(sortListRows(rows, sort))).toEqual(["First", "Second", "Unordered", "Also unordered"]);
  });

  it("ignores the direction for manual order", () => {
    const rows = [project("B", { sort_order: 200 }), project("A", { sort_order: 100 })];
    expect(names(sortListRows(rows, { key: "manual", direction: "descending" }))).toEqual(["A", "B"]);
  });

  it("keeps collection order for rows sharing a manual slot", () => {
    const rows = [project("Second", { sort_order: 100 }), project("First", { sort_order: 100 })];
    expect(names(sortListRows(rows, { key: "manual", direction: "ascending" }))).toEqual(["Second", "First"]);
  });

  it("orders names case-insensitively and stably", () => {
    const rows = [
      project("delta", { id: "a" }),
      project("Bravo", { id: "b" }),
      project("alpha", { id: "c" }),
      project("ALPHA", { id: "d" }),
    ];
    expect(names(sortListRows(rows, { key: "name", direction: "ascending" }))).toEqual([
      "alpha",
      "ALPHA",
      "Bravo",
      "delta",
    ]);
  });

  it("reverses names for descending", () => {
    const rows = [project("Bravo"), project("alpha")];
    expect(names(sortListRows(rows, { key: "name", direction: "descending" }))).toEqual(["Bravo", "alpha"]);
  });

  it("orders creation oldest first and newest first", () => {
    const rows = [
      project("Middle", { created_at: "2024-06-01T00:00:00Z" }),
      project("Oldest", { created_at: "2024-01-01T00:00:00Z" }),
      project("Newest", { created_at: "2024-12-01T00:00:00Z" }),
      project("Twin", { created_at: "2024-01-01T00:00:00Z" }),
    ];
    expect(names(sortListRows(rows, { key: "created", direction: "ascending" }))).toEqual([
      "Oldest",
      "Twin",
      "Middle",
      "Newest",
    ]);
    expect(names(sortListRows(rows, { key: "created", direction: "descending" }))).toEqual([
      "Newest",
      "Middle",
      "Twin",
      "Oldest",
    ]);
  });

  it("orders member counts with ties keeping collection order", () => {
    const rows = [
      project("Two", { members: ["u1", "u2"] }),
      project("Zero", { members: [] }),
      project("One", { members: ["u1"] }),
      project("Two again", { members: ["u3", "u4"] }),
    ];
    expect(names(sortListRows(rows, { key: "members", direction: "ascending" }))).toEqual([
      "Zero",
      "One",
      "Two",
      "Two again",
    ]);
    expect(names(sortListRows(rows, { key: "members", direction: "descending" }))).toEqual([
      "Two again",
      "Two",
      "One",
      "Zero",
    ]);
  });
});

describe("ACCESS_OPTIONS", () => {
  it("covers private and public networks", () => {
    expect(ACCESS_OPTIONS).toEqual([
      { value: 0, label: "Private" },
      { value: 2, label: "Public" },
    ]);
  });
});

describe("matchesCreatedFilter", () => {
  const today = new Date(2026, 9, 14);

  it("matches the day presets against today", () => {
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "today;custom;custom", today)).toBe(true);
    expect(matchesCreatedFilter("2026-10-13T03:00:00Z", "today;custom;custom", today)).toBe(false);
    expect(matchesCreatedFilter("2026-10-13T03:00:00Z", "yesterday;custom;custom", today)).toBe(true);
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "yesterday;custom;custom", today)).toBe(false);
  });

  it("matches the rolling windows including the boundary day", () => {
    expect(matchesCreatedFilter("2026-10-07T03:00:00Z", "last_7_days;custom;custom", today)).toBe(true);
    expect(matchesCreatedFilter("2026-10-06T03:00:00Z", "last_7_days;custom;custom", today)).toBe(false);
    expect(matchesCreatedFilter("2026-09-14T03:00:00Z", "last_30_days;custom;custom", today)).toBe(true);
    expect(matchesCreatedFilter("2026-09-13T03:00:00Z", "last_30_days;custom;custom", today)).toBe(false);
  });

  it("rejects unknown preset values", () => {
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "last_year;custom;custom", today)).toBe(false);
  });

  it("matches absolute picks with inclusive day boundaries", () => {
    expect(matchesCreatedFilter("2026-10-10T23:59:59Z", "2026-10-10;after", today)).toBe(true);
    expect(matchesCreatedFilter("2026-10-09T00:00:00Z", "2026-10-10;after", today)).toBe(false);
    expect(matchesCreatedFilter("2026-10-10T00:00:00Z", "2026-10-10;before", today)).toBe(true);
    expect(matchesCreatedFilter("2026-10-11T00:00:00Z", "2026-10-10;before", today)).toBe(false);
  });

  it("rejects malformed values and timestamps", () => {
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "today;custom", today)).toBe(false);
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "2026-10-10;during", today)).toBe(false);
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "someday;after", today)).toBe(false);
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "2026-10-10", today)).toBe(false);
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "2026-10-10;after;fromnow", today)).toBe(false);
    expect(matchesCreatedFilter("not-a-date", "today;custom;custom", today)).toBe(false);
    expect(matchesCreatedFilter("2026-13-40T03:00:00Z", "today;custom;custom", today)).toBe(false);
    expect(matchesCreatedFilter("2026-02-30T03:00:00Z", "today;custom;custom", today)).toBe(false);
    expect(matchesCreatedFilter("0099-01-01T03:00:00Z", "today;custom;custom", today)).toBe(false);
  });

  it("accepts a trailing separator on absolute picks", () => {
    expect(matchesCreatedFilter("2026-10-14T03:00:00Z", "2026-10-10;after;", today)).toBe(true);
    expect(matchesCreatedFilter("2026-10-09T03:00:00Z", "2026-10-10;after;", today)).toBe(false);
  });
});

describe("formatCreatedChip", () => {
  it("names presets", () => {
    expect(CREATED_PRESETS.map((preset) => preset.label)).toEqual([
      "Today",
      "Yesterday",
      "Last 7 days",
      "Last 30 days",
    ]);
    expect(formatCreatedChip("last_7_days;custom;custom")).toBe("Last 7 days");
  });

  it("phrases absolute picks as After/Before days", () => {
    expect(formatCreatedChip("2026-10-01;after")).toBe("After Oct 01, 2026");
    expect(formatCreatedChip("2026-01-31;before")).toBe("Before Jan 31, 2026");
  });

  it("blanks malformed values", () => {
    expect(formatCreatedChip("2026-10-01;during")).toBe("");
    expect(formatCreatedChip("someday;after")).toBe("");
    expect(formatCreatedChip("2026-10-01")).toBe("");
  });
});

describe("isCustomCreatedValue", () => {
  it("tells presets from absolute picks", () => {
    expect(isCustomCreatedValue("today;custom;custom")).toBe(false);
    expect(isCustomCreatedValue("2026-10-01;after")).toBe(true);
  });
});

describe("matchesListFilters", () => {
  it("passes everything when no filter narrows", () => {
    expect(EMPTY_LIST_FILTERS).toEqual({ myProjects: false, access: [], lead: [], members: [], createdAt: [] });
    expect(matchesListFilters(project("A"), filters())).toBe(true);
  });

  it("matches access by network", () => {
    const secret = project("Secret", { network: 0 });
    expect(matchesListFilters(secret, filters({ access: [0] }))).toBe(true);
    expect(matchesListFilters(secret, filters({ access: [2] }))).toBe(false);
    expect(matchesListFilters(secret, filters({ access: [0, 2] }))).toBe(true);
  });

  it("matches lead by member id", () => {
    const led = project("Led", { project_lead: "u1" });
    expect(matchesListFilters(led, filters({ lead: ["u1"] }))).toBe(true);
    expect(matchesListFilters(led, filters({ lead: ["u2"] }))).toBe(false);
    expect(matchesListFilters(project("Unled"), filters({ lead: ["u1"] }))).toBe(false);
  });

  it("matches members on any overlap", () => {
    const teamed = project("Teamed", { members: ["u2", "u3"] });
    expect(matchesListFilters(teamed, filters({ members: ["u1", "u3"] }))).toBe(true);
    expect(matchesListFilters(teamed, filters({ members: ["u1"] }))).toBe(false);
  });

  it("matches creation dates with every pick", () => {
    const fresh = project("Fresh", { created_at: "2026-10-14T03:00:00Z" });
    expect(matchesListFilters(fresh, filters({ createdAt: ["2026-10-01;after", "2026-10-31;before"] }))).toBe(true);
    expect(matchesListFilters(fresh, filters({ createdAt: ["2026-10-15;after"] }))).toBe(false);
  });

  it("keeps member rows for My projects", () => {
    expect(matchesListFilters(project("Mine", { member_role: 15 }), filters({ myProjects: true }))).toBe(true);
    expect(matchesListFilters(project("Theirs", { member_role: null }), filters({ myProjects: true }))).toBe(false);
  });

  it("combines dimensions with AND", () => {
    const row = project("Row", { network: 0, project_lead: "u1" });
    expect(matchesListFilters(row, filters({ access: [0], lead: ["u1"] }))).toBe(true);
    expect(matchesListFilters(row, filters({ access: [0], lead: ["u2"] }))).toBe(false);
  });
});

describe("matchesListSearch", () => {
  it("matches names and identifiers case-insensitively", () => {
    const row = project("Findable Apple", { identifier: "FAP" });
    expect(matchesListSearch(row, "")).toBe(true);
    expect(matchesListSearch(row, "apple")).toBe(true);
    expect(matchesListSearch(row, "APPLE")).toBe(true);
    expect(matchesListSearch(row, "fap")).toBe(true);
    expect(matchesListSearch(row, "banana")).toBe(false);
  });
});

describe("filterListRows", () => {
  it("applies search and filters together", () => {
    const rows = [
      project("Apple Orchard", { network: 0 }),
      project("Apple Storage", { network: 2 }),
      project("Banana Stand", { network: 0 }),
    ];
    expect(filterListRows(rows, filters(), "apple").map((row) => row.name)).toEqual(["Apple Orchard", "Apple Storage"]);
    expect(filterListRows(rows, filters({ access: [0] }), "apple").map((row) => row.name)).toEqual(["Apple Orchard"]);
  });
});

describe("filter counts", () => {
  it("badges panel picks without My projects", () => {
    expect(countActiveListFilters(filters())).toBe(0);
    expect(countActiveListFilters(filters({ myProjects: true }))).toBe(0);
    expect(countActiveListFilters(filters({ access: [0], lead: ["u1"], members: [], createdAt: ["a", "b"] }))).toBe(4);
  });

  it("flags narrowing for the empty-state decision", () => {
    expect(hasNarrowingListFilters(filters())).toBe(false);
    expect(hasNarrowingListFilters(filters({ myProjects: true }))).toBe(true);
    expect(hasNarrowingListFilters(filters({ access: [0] }))).toBe(true);
  });
});

describe("selectMemberOptions", () => {
  const memberships = [member("u2", "Zed Shaw"), member("u1", "Ada Lovelace"), member("u3", "ada Ng")];

  it("sorts applied first, then the caller, then names", () => {
    const options = selectMemberOptions(memberships, { applied: ["u2"], selfId: "u3", query: "" });
    expect(options.map((option) => option.id)).toEqual(["u2", "u3", "u1"]);
    expect(options[1]).toMatchObject({ displayName: "ada Ng", self: true, applied: false });
    expect(options[2]).toMatchObject({ self: false, applied: false });
    expect(options[0]?.avatarUrl).toBe("https://cdn.test/u2.png");
  });

  it("searches display names case-insensitively", () => {
    const options = selectMemberOptions(memberships, { applied: [], selfId: null, query: "ADA" });
    expect(options.map((option) => option.id)).toEqual(["u1", "u3"]);
    expect(options.every((option) => !option.self)).toBe(true);
  });

  it("matches nothing on a nonsense query", () => {
    expect(selectMemberOptions(memberships, { applied: [], selfId: "u1", query: "zzz" })).toEqual([]);
  });

  it("excludes bots from the options", () => {
    const bot = member("u9", "Helper Bot");
    bot.member.is_bot = true;
    const options = selectMemberOptions([...memberships, bot], { applied: [], selfId: null, query: "" });
    expect(options.map((option) => option.id)).toEqual(["u1", "u3", "u2"]);
  });

  it("maps a missing avatar to an empty source", () => {
    const noAvatar = member("u4", "No Avatar");
    noAvatar.member.avatar_url = null;
    const options = selectMemberOptions([noAvatar], { applied: [], selfId: null, query: "" });
    expect(options[0]?.avatarUrl).toBe("");
  });
});
