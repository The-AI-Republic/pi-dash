// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { SavedView } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  buildViewCreate,
  buildViewPatch,
  entityViewContext,
  isViewDirty,
  offersSaveAs,
  offersSaveView,
  offersUpdateView,
  stableJson,
  viewContextFromRecord,
  viewDisplaySnapshot,
  type ViewContext,
  type ViewSnapshot,
} from "./views.js";
import { parseDisplayPrefs } from "../../prefs/displayPrefs.js";

const VIEW_ID = "123e4567-e89b-12d3-a456-426614174000";
const OWNER = "223e4567-e89b-12d3-a456-426614174001";
const PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const WORKSPACE = "423e4567-e89b-12d3-a456-426614174003";

const SNAPSHOT: ViewSnapshot = {
  expression: { priority__in: "urgent" },
  displayFilters: { layout: "list", group_by: null },
  displayProperties: { priority: true },
};

function record(overrides: Partial<SavedView> = {}): SavedView {
  return {
    id: VIEW_ID,
    workspace: WORKSPACE,
    project: PROJECT,
    name: "Mine",
    description: "",
    query: {},
    filters: {},
    display_filters: { layout: "list", group_by: null },
    display_properties: { priority: true },
    rich_filters: { priority__in: "urgent" },
    access: 0,
    sort_order: 1,
    logo_props: {},
    owned_by: OWNER,
    is_locked: false,
    archived_at: null,
    created_by: OWNER,
    updated_by: OWNER,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function saved(overrides: Partial<SavedView> = {}, isDefault = false): ViewContext {
  return viewContextFromRecord(record(overrides), OWNER, { isDefault });
}

describe("buildViewCreate", () => {
  it("carries the name and the three snapshot keys", () => {
    expect(buildViewCreate(SNAPSHOT, { name: "Mine" })).toEqual({
      name: "Mine",
      rich_filters: SNAPSHOT.expression,
      display_filters: SNAPSHOT.displayFilters,
      display_properties: SNAPSHOT.displayProperties,
    });
  });

  it("includes a non-empty description and omits an empty one", () => {
    expect(buildViewCreate(SNAPSHOT, { name: "Mine", description: "Team triage" })["description"]).toBe("Team triage");
    expect(buildViewCreate(SNAPSHOT, { name: "Mine", description: "" })).not.toHaveProperty("description");
  });
});

describe("buildViewPatch", () => {
  it("patches only the snapshot keys", () => {
    expect(buildViewPatch(SNAPSHOT)).toEqual({
      rich_filters: SNAPSHOT.expression,
      display_filters: SNAPSHOT.displayFilters,
      display_properties: SNAPSHOT.displayProperties,
    });
  });
});

describe("save offers", () => {
  it("offers Save view on a bare list and Save as on an unlocked saved view", () => {
    expect(offersSaveView(entityViewContext())).toBe(true);
    expect(offersSaveAs(entityViewContext())).toBe(false);
    expect(offersSaveView(saved())).toBe(false);
    expect(offersSaveAs(saved())).toBe(true);
  });

  it("refuses Save as on a locked view", () => {
    expect(offersSaveAs(saved({ is_locked: true }))).toBe(false);
  });
});

describe("offersUpdateView", () => {
  it("requires the owner of an unlocked, non-default, changed view", () => {
    const changed: ViewSnapshot = { ...SNAPSHOT, expression: { priority__in: "high" } };
    expect(offersUpdateView(saved(), changed, OWNER)).toBe(true);
  });

  it("refuses a clean view", () => {
    const context = saved();
    expect(offersUpdateView(context, SNAPSHOT, OWNER)).toBe(false);
  });

  it("refuses a different owner", () => {
    expect(offersUpdateView(saved(), SNAPSHOT, "someone-else")).toBe(false);
    expect(offersUpdateView(saved({ owned_by: null as never }), SNAPSHOT, OWNER)).toBe(false);
    expect(offersUpdateView(saved(), SNAPSHOT, null)).toBe(false);
  });

  it("refuses locked, default and bare-list contexts", () => {
    expect(offersUpdateView(saved({ is_locked: true }), SNAPSHOT, OWNER)).toBe(false);
    expect(offersUpdateView(saved({}, true), SNAPSHOT, OWNER)).toBe(false);
    expect(offersUpdateView(entityViewContext(), SNAPSHOT, OWNER)).toBe(false);
    expect(offersUpdateView(saved({ id: null as never }), SNAPSHOT, OWNER)).toBe(false);
  });

  it("notices a change in any of the three stored keys", () => {
    expect(offersUpdateView(saved(), { ...SNAPSHOT, expression: { state_id__in: ["x"] } }, OWNER)).toBe(true);
    expect(offersUpdateView(saved(), { ...SNAPSHOT, displayFilters: { layout: "board" } }, OWNER)).toBe(true);
    expect(offersUpdateView(saved(), { ...SNAPSHOT, displayProperties: { priority: false } }, OWNER)).toBe(true);
  });
});

describe("isViewDirty", () => {
  it("ignores key order", () => {
    const context = saved();
    const reordered: ViewSnapshot = {
      expression: { priority__in: "urgent" },
      displayFilters: { group_by: null, layout: "list" },
      displayProperties: { priority: true },
    };
    expect(isViewDirty(context, reordered)).toBe(false);
  });

  it("treats nested differences and type changes as dirty", () => {
    const context = saved({ rich_filters: { and: [{ priority__in: "urgent" }] } });
    expect(isViewDirty(context, { ...SNAPSHOT, expression: { and: [{ priority__in: "high" }] } })).toBe(true);
    expect(isViewDirty(context, { ...SNAPSHOT, expression: { priority__in: ["urgent"] } })).toBe(true);
  });

  it("sorts keys inside arrays positionally", () => {
    expect(stableJson({ b: 1, a: [{ d: 1, c: 2 }] })).toBe('{"a":[{"c":2,"d":1}],"b":1}');
    expect(stableJson(null)).toBe("null");
  });
});

describe("viewContextFromRecord", () => {
  it("derives the project scope and copies the stored keys", () => {
    const context = saved();
    expect(context).toMatchObject({
      isView: true,
      viewScope: "project",
      viewId: VIEW_ID,
      ownerId: OWNER,
      locked: false,
      isDefault: false,
      storedExpression: { priority__in: "urgent" },
    });
  });

  it("derives the workspace scope for a view without a project", () => {
    expect(saved({ project: null }).viewScope).toBe("workspace");
  });
});

describe("entityViewContext", () => {
  it("is a bare project list with no record", () => {
    expect(entityViewContext()).toEqual({
      isView: false,
      viewId: null,
      ownerId: null,
      locked: false,
      isDefault: false,
      viewScope: "project",
      storedExpression: {},
      storedDisplayFilters: {},
      storedDisplayProperties: {},
    });
  });
});

describe("viewDisplaySnapshot", () => {
  it("overwrites the stored display keys while keeping unknown ones", () => {
    const view = {
      display_filters: {
        layout: "list",
        group_by: "state",
        order_by: "sort_order",
        calendar: { layout: "week" },
        custom_key: "kept",
      },
      display_properties: { priority: true, custom: false },
    };
    const snapshot = viewDisplaySnapshot(view, {
      layout: "gantt_chart",
      groupBy: null,
      subGroupBy: null,
      orderBy: "-created_at",
      showEmptyGroups: true,
      showSubIssues: false,
      calendar: { mode: "month", showWeekends: false },
      properties: { priority: false },
    });
    expect(snapshot.displayFilters).toEqual({
      layout: "gantt_chart",
      group_by: null,
      sub_group_by: null,
      order_by: "-created_at",
      show_empty_groups: true,
      sub_issue: false,
      calendar: { layout: "month", show_weekends: false },
      custom_key: "kept",
    });
    expect(snapshot.displayProperties).toEqual({ priority: false, custom: false });
  });

  it("round-trips: the snapshot parses back to the settings it stored", () => {
    const prefs = parseDisplayPrefs({ layout: "gantt_chart", order_by: "-updated_at" }, { priority: false });
    const snapshot = viewDisplaySnapshot({ display_filters: {}, display_properties: {} }, prefs);
    expect(parseDisplayPrefs(snapshot.displayFilters, snapshot.displayProperties)).toEqual(prefs);
  });
});
