// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace views list model (parity/specs/views.md §9, VIEW-029-032/039).
import { describe, expect, it } from "vitest";

import { makeView } from "../testing.js";
import {
  capWorkspaceViewName,
  matchesWorkspaceViewQuery,
  resolveWorkspaceCustomsState,
  selectWorkspaceViewGroups,
  sortWorkspaceViewsNewestFirst,
  WORKSPACE_DEFAULT_VIEWS,
  WORKSPACE_VIEW_NAME_LIMIT,
  WORKSPACE_VIEWS_CRUMB,
  workspaceCustomViewHref,
  workspaceDefaultViewHref,
  workspaceViewsListHref,
  workspaceViewsTitle,
} from "./workspaceViews.js";

describe("WORKSPACE_DEFAULT_VIEWS", () => {
  it("lists the four static views in their fixed order", () => {
    expect(WORKSPACE_DEFAULT_VIEWS).toEqual([
      { key: "all-issues", label: "All work items" },
      { key: "assigned", label: "Assigned" },
      { key: "created", label: "Created" },
      { key: "subscribed", label: "Subscribed" },
    ]);
  });
});

describe("workspaceViewsTitle", () => {
  it("joins the workspace name and the surface name", () => {
    expect(workspaceViewsTitle("Acme")).toBe("Acme - All Views");
  });

  it("falls back to the surface name alone before the workspace loads", () => {
    expect(workspaceViewsTitle(undefined)).toBe("All Views");
    expect(workspaceViewsTitle("")).toBe("All Views");
  });
});

describe("hrefs", () => {
  it("links a default by its static key", () => {
    expect(workspaceDefaultViewHref("acme", "assigned")).toBe("/acme/workspace-views/assigned");
  });

  it("links a custom by its id", () => {
    expect(workspaceCustomViewHref("acme", "view-9")).toBe("/acme/workspace-views/view-9");
  });

  it("links the list for the detail crumb", () => {
    expect(workspaceViewsListHref("acme")).toBe("/acme/workspace-views");
  });

  it("names the list crumb", () => {
    expect(WORKSPACE_VIEWS_CRUMB).toBe("Views");
  });
});

describe("matchesWorkspaceViewQuery", () => {
  it("keeps everything for a blank or whitespace query", () => {
    expect(matchesWorkspaceViewQuery("Anything", "")).toBe(true);
    expect(matchesWorkspaceViewQuery("Anything", "   ")).toBe(true);
  });

  it("matches a case-insensitive substring", () => {
    expect(matchesWorkspaceViewQuery("Assigned", "assign")).toBe(true);
    expect(matchesWorkspaceViewQuery("Assigned", "SIGN")).toBe(true);
    expect(matchesWorkspaceViewQuery("Assigned", "zzz")).toBe(false);
  });

  it("treats regex metacharacters literally", () => {
    expect(matchesWorkspaceViewQuery("v1.2 (final)", "1.2 (")).toBe(true);
    expect(matchesWorkspaceViewQuery("anything", ".*")).toBe(false);
  });

  it("ignores surrounding whitespace in the query", () => {
    expect(matchesWorkspaceViewQuery("Created", "  created  ")).toBe(true);
  });
});

describe("capWorkspaceViewName", () => {
  it("leaves a short name untouched", () => {
    expect(capWorkspaceViewName("Triage")).toBe("Triage");
  });

  it("keeps a name exactly at the limit", () => {
    const exact = "x".repeat(WORKSPACE_VIEW_NAME_LIMIT);
    expect(capWorkspaceViewName(exact)).toBe(exact);
  });

  it("shortens a long name and adds an ellipsis", () => {
    const long = "n".repeat(120);
    const capped = capWorkspaceViewName(long);
    expect(capped.length).toBe(WORKSPACE_VIEW_NAME_LIMIT);
    expect(capped.endsWith("…")).toBe(true);
    expect(capped).toBe(`${"n".repeat(WORKSPACE_VIEW_NAME_LIMIT - 1)}…`);
  });

  it("does not split a surrogate pair at the cut", () => {
    const emoji = "😀";
    const name = "a".repeat(WORKSPACE_VIEW_NAME_LIMIT - 2) + emoji + "tail";
    const capped = capWorkspaceViewName(name);
    expect(capped).toBe(`${"a".repeat(WORKSPACE_VIEW_NAME_LIMIT - 2)}…`);
  });

  it("trims a trailing space before the ellipsis", () => {
    const name = `${"a".repeat(WORKSPACE_VIEW_NAME_LIMIT - 2)} b${"c".repeat(20)}`;
    expect(capWorkspaceViewName(name)).toBe(`${"a".repeat(WORKSPACE_VIEW_NAME_LIMIT - 2)}…`);
  });

  it("accepts an explicit limit", () => {
    expect(capWorkspaceViewName("abcdef", 4)).toBe("abc…");
  });
});

describe("sortWorkspaceViewsNewestFirst", () => {
  it("orders by creation time, newest first, without mutating the input", () => {
    const older = makeView({ id: "a", name: "Older", created_at: "2026-01-01T00:00:00Z" });
    const newer = makeView({ id: "b", name: "Newer", created_at: "2026-03-01T00:00:00Z" });
    const input = [older, newer];
    expect(sortWorkspaceViewsNewestFirst(input).map((view) => view.id)).toEqual(["b", "a"]);
    expect(input.map((view) => view.id)).toEqual(["a", "b"]);
  });

  it("keeps the fetched order for equal stamps", () => {
    const first = makeView({ id: "a", created_at: "2026-01-01T00:00:00Z" });
    const second = makeView({ id: "b", created_at: "2026-01-01T00:00:00Z" });
    expect(sortWorkspaceViewsNewestFirst([first, second]).map((view) => view.id)).toEqual(["a", "b"]);
  });
});

describe("selectWorkspaceViewGroups", () => {
  const custom = (id: string, name: string) => makeView({ id, name });

  it("returns both groups unfiltered for a blank query", () => {
    const groups = selectWorkspaceViewGroups([custom("a", "Triage"), custom("b", "Bugs")], "");
    expect(groups.defaults.map((view) => view.key)).toEqual(["all-issues", "assigned", "created", "subscribed"]);
    expect(groups.customs.map((view) => view.id)).toEqual(["a", "b"]);
  });

  it("filters each group by name, hiding a group with no match", () => {
    const onlyCustoms = selectWorkspaceViewGroups([custom("a", "Triage"), custom("b", "Bugs")], "tri");
    expect(onlyCustoms.defaults).toEqual([]);
    expect(onlyCustoms.customs.map((view) => view.id)).toEqual(["a"]);

    const onlyDefaults = selectWorkspaceViewGroups([custom("a", "Triage")], "assigned");
    expect(onlyDefaults.defaults.map((view) => view.label)).toEqual(["Assigned"]);
    expect(onlyDefaults.customs).toEqual([]);
  });

  it("matches defaults and customs together when both contain the term", () => {
    const groups = selectWorkspaceViewGroups([custom("a", "My created board")], "created");
    expect(groups.defaults.map((view) => view.key)).toEqual(["created"]);
    expect(groups.customs.map((view) => view.id)).toEqual(["a"]);
  });
});

describe("resolveWorkspaceCustomsState", () => {
  it("shows the skeleton while pending, even if a retry also failed previously", () => {
    expect(resolveWorkspaceCustomsState(true, false)).toBe("loading");
    expect(resolveWorkspaceCustomsState(true, true)).toBe("loading");
  });

  it("shows the error state after a failed read", () => {
    expect(resolveWorkspaceCustomsState(false, true)).toBe("error");
  });

  it("shows rows for a settled read", () => {
    expect(resolveWorkspaceCustomsState(false, false)).toBe("rows");
  });
});
