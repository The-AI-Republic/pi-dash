// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import type { Edition } from "../../../core/edition/index.js";
import { WORKSPACE_DEFAULT_VIEWS } from "../lib/workspaceViews.js";
import { makeView } from "../testing.js";
import {
  findVisibleCustomView,
  findWorkspaceDefaultView,
  offeredWorkspaceLayouts,
  resolveWorkspaceBodyLayout,
  workspaceActiveLayout,
  workspaceDefaultStaticFilters,
  workspaceLayoutPatch,
} from "./workspaceDetail.js";

const ME = "123e4567-e89b-12d3-a456-426614174000";

function cloudEdition(layouts: readonly string[]): Edition {
  return { id: "cloud", flags: {}, slots: { workspaceViewLayouts: { layouts } } };
}

describe("findWorkspaceDefaultView", () => {
  it.each(["all-issues", "assigned", "created", "subscribed"])("resolves the %s key", (key) => {
    expect(findWorkspaceDefaultView(key)?.key).toBe(key);
  });

  it("returns null for a custom id", () => {
    expect(findWorkspaceDefaultView("223e4567-e89b-12d3-a456-426614174001")).toBeNull();
  });
});

describe("findVisibleCustomView", () => {
  const views = [makeView({ id: "223e4567-e89b-12d3-a456-426614174001", name: "Alpha" })];

  it("returns the listed record", () => {
    const [first] = views;
    if (!first) throw new Error("missing fixture view");
    expect(findVisibleCustomView(views, first.id)?.name).toBe("Alpha");
  });

  it("returns null when the list hides the id", () => {
    expect(findVisibleCustomView(views, "323e4567-e89b-12d3-a456-426614174002")).toBeNull();
  });
});

describe("workspaceDefaultStaticFilters", () => {
  it.each([
    ["assigned", { assignees: ME }],
    ["created", { created_by: ME }],
    ["subscribed", { subscriber: ME }],
  ])("conditions the %s view on the current user", (key, filters) => {
    const view = WORKSPACE_DEFAULT_VIEWS.find((entry) => entry.key === key);
    if (!view) throw new Error(`missing default ${key}`);
    expect(workspaceDefaultStaticFilters(view, ME)).toEqual(filters);
  });

  it("adds no condition for all-issues", () => {
    const view = WORKSPACE_DEFAULT_VIEWS.find((entry) => entry.key === "all-issues");
    if (!view) throw new Error("missing all-issues default");
    expect(workspaceDefaultStaticFilters(view, ME)).toBeNull();
  });
});

describe("offeredWorkspaceLayouts", () => {
  it("offers nothing when the slot is absent (OSS)", () => {
    expect(offeredWorkspaceLayouts({ id: "oss", flags: {} })).toEqual([]);
  });

  it("keeps known keys in the slot's order and drops unknown ones", () => {
    expect(offeredWorkspaceLayouts(cloudEdition(["kanban", "bogus", "list"]))).toEqual(["kanban", "list"]);
  });
});

describe("resolveWorkspaceBodyLayout", () => {
  it.each([undefined, null])("opens the spreadsheet when nothing is stored (%s)", (stored) => {
    expect(resolveWorkspaceBodyLayout(stored, [])).toBe("spreadsheet");
  });

  it("renders the stored spreadsheet", () => {
    expect(resolveWorkspaceBodyLayout("spreadsheet", [])).toBe("spreadsheet");
  });

  it("renders a cloud-offered layout", () => {
    expect(resolveWorkspaceBodyLayout("kanban", ["kanban", "list"])).toBe("kanban");
  });

  it("renders nothing for a stored layout the surface cannot draw", () => {
    expect(resolveWorkspaceBodyLayout("kanban", [])).toBeNull();
    expect(resolveWorkspaceBodyLayout("kanban", ["list"])).toBeNull();
  });

  it("renders nothing for an unknown stored value", () => {
    expect(resolveWorkspaceBodyLayout("bogus", ["kanban"])).toBeNull();
  });
});

describe("workspaceActiveLayout", () => {
  it("marks the stored layout", () => {
    expect(workspaceActiveLayout(makeView({ display_filters: { layout: "kanban" } }))).toBe("kanban");
  });

  it.each([undefined, null, "bogus"])("falls back to the spreadsheet for %s", (layout) => {
    expect(workspaceActiveLayout(makeView({ display_filters: { layout } }))).toBe("spreadsheet");
  });
});

describe("workspaceLayoutPatch", () => {
  it("carries the switch beside the stored display keys", () => {
    expect(
      workspaceLayoutPatch(makeView({ display_filters: { layout: "spreadsheet", order_by: "-created_at" } }), "kanban")
    ).toEqual({ display_filters: { layout: "kanban", order_by: "-created_at" } });
  });
});
