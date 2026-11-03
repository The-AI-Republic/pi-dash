// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { selectProjectMark, selectProjectOptions, toProjectOption } from "./projects.js";

const SEED = "22222222-2222-4222-8222-222222222222";
const OTHER = "33333333-3333-4333-8333-333333333333";
const WORKSPACE = "44444444-4444-4444-8444-444444444444";

function projectRow(id: string, name: string, identifier: string) {
  return {
    id,
    name,
    identifier,
    sort_order: null,
    logo_props: {},
    member_role: 15,
    intake_count: 0,
    archived_at: null,
    workspace: WORKSPACE,
    cycle_view: false,
    issue_views_view: false,
    module_view: false,
    page_view: false,
    inbox_view: false,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 0,
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    created_by: null,
    updated_by: null,
  };
}

describe("selectProjectOptions", () => {
  it("sorts selected-first and keeps server order otherwise", () => {
    const options = selectProjectOptions({
      projects: [projectRow(SEED, "Seed", "SEED"), projectRow(OTHER, "Other", "OTH")],
      selectedIds: [OTHER],
    });
    expect(options.map((option) => option.id)).toEqual([OTHER, SEED]);
    expect(options[0]).toMatchObject({ name: "Other", searchText: "Other OTH", selected: true });
  });

  it("keeps the current project listed unless excluded", () => {
    const listed = selectProjectOptions({ projects: [projectRow(SEED, "Seed", "SEED")], selectedIds: [SEED] });
    expect(listed.map((option) => option.id)).toEqual([SEED]);
    const excluded = selectProjectOptions({
      projects: [projectRow(SEED, "Seed", "SEED"), projectRow(OTHER, "Other", "OTH")],
      excludeIds: [SEED],
      selectedIds: [],
    });
    expect(excluded.map((option) => option.id)).toEqual([OTHER]);
  });

  it("never offers a row outside the caller's allow list", () => {
    const rows = [projectRow(SEED, "Seed", "SEED"), projectRow(OTHER, "Other", "OTH")];
    expect(selectProjectOptions({ projects: rows, includeIds: [OTHER], selectedIds: [] }).map((o) => o.id)).toEqual([
      OTHER,
    ]);
    expect(selectProjectOptions({ projects: rows, includeIds: [], selectedIds: [SEED] })).toEqual([]);
    expect(selectProjectOptions({ projects: rows, includeIds: [SEED], excludeIds: [SEED], selectedIds: [] })).toEqual(
      []
    );
    expect(selectProjectOptions({ projects: rows, selectedIds: [] }).map((o) => o.id)).toEqual([SEED, OTHER]);
  });
});

describe("toProjectOption", () => {
  it("shapes a display row from a partial project", () => {
    expect(toProjectOption({ id: SEED, name: "Atlas", identifier: "ATL" })).toEqual({
      id: SEED,
      name: "Atlas",
      identifier: "ATL",
      logoProps: {},
      searchText: "Atlas ATL",
      selected: false,
    });
  });

  it("keeps logo props and never marks itself selected", () => {
    const option = toProjectOption({ id: SEED, name: "Atlas", identifier: "ATL" }, { in_use: "emoji" });
    expect(option.logoProps).toEqual({ in_use: "emoji" });
    expect(option.selected).toBe(false);
    expect(selectProjectMark(option.logoProps, option.identifier)).toBe("A");
  });
});

describe("selectProjectMark", () => {
  it("prefers the picked emoji glyph", () => {
    expect(selectProjectMark({ in_use: "emoji", emoji: { value: "🚀" } }, "SEED")).toBe("🚀");
  });

  it("falls back to the identifier initial", () => {
    expect(selectProjectMark({}, "seed")).toBe("S");
    expect(selectProjectMark({ in_use: "icon" }, "seed")).toBe("S");
    expect(selectProjectMark({ in_use: "emoji", emoji: null }, "seed")).toBe("S");
    expect(selectProjectMark({ in_use: "emoji", emoji: { value: "  " } }, "seed")).toBe("S");
    expect(selectProjectMark({ in_use: "emoji", emoji: { value: 7 } }, "seed")).toBe("S");
    expect(selectProjectMark({ in_use: "emoji" }, "")).toBe("?");
  });
});
