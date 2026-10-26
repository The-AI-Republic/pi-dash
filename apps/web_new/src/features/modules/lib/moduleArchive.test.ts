// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive/restore copy and the archived screen's decisions
// (parity/specs/modules.md §7.3–§7.6, MOD-026/027/028): the dialog names the
// module and promises a restore, the notices say where the module went, the
// body decision keeps loader/error/three empties/rows apart, the archived
// narrowing drops status and favorites, and an archived row reads in the
// shared row shape.

import { describe, expect, it } from "vitest";

import type { ArchivedModuleListItem } from "@pidash/api-client";

import {
  ARCHIVED_MODULE_FILTER_DIMENSIONS,
  archivedModuleFilters,
  archivedModulesSearched,
  archivedModulesVisible,
  hasArchivedModuleFilters,
  MODULE_ARCHIVE_CONFIRM,
  MODULE_ARCHIVE_HEADING,
  MODULE_ARCHIVE_SUCCESS_BODY,
  MODULE_ARCHIVE_SUCCESS_TITLE,
  moduleArchiveDialogBody,
  moduleArchiveDialogTitle,
  moduleArchiveLanding,
  resolveArchivedModulesBody,
  toArchivedModuleRow,
  type ArchivedModulesBodyInput,
} from "./moduleArchive.js";
import { emptyModuleFilters, type ModuleListFilters } from "./moduleListControls.js";
import { MODULES_MODULE_ID, MODULES_PROJECT_ID, makeModuleRow } from "../testing.js";

function body(overrides: Partial<ArchivedModulesBodyInput> = {}): ArchivedModulesBodyInput {
  return { listPending: false, totalCount: 3, searchedCount: 3, visibleCount: 3, ...overrides };
}

describe("archive copy", () => {
  it("names the module in the dialog title under the pinned heading", () => {
    expect(MODULE_ARCHIVE_HEADING).toBe("Archive module");
    expect(moduleArchiveDialogTitle("Atlas")).toBe("Archive module “Atlas”");
  });

  it("says the archive can be restored later", () => {
    const copy = moduleArchiveDialogBody("Atlas");
    expect(copy).toContain("Atlas");
    expect(copy).toContain("restored");
  });

  it("pins the success notice and points at the project archives", () => {
    expect(MODULE_ARCHIVE_SUCCESS_TITLE).toBe("Archive success");
    expect(MODULE_ARCHIVE_SUCCESS_BODY).toContain("project's archives");
    expect(MODULE_ARCHIVE_CONFIRM).toBe("Archive");
  });

  it("lands a landed archive back on the modules list", () => {
    expect(moduleArchiveLanding("acme", MODULES_PROJECT_ID)).toEqual({
      to: `/acme/projects/${MODULES_PROJECT_ID}/modules/`,
      search: {},
      replace: true,
    });
  });
});

describe("resolveArchivedModulesBody", () => {
  it("prefers the skeleton while the read is in flight", () => {
    expect(resolveArchivedModulesBody(body({ listPending: true, totalCount: 0 }))).toBe("skeleton");
  });

  it("offers the error state rather than an empty archive on a failed read", () => {
    expect(resolveArchivedModulesBody(body({ listError: true, totalCount: 0 }))).toBe("error");
  });

  it("shows the zero state for an empty archive", () => {
    expect(resolveArchivedModulesBody(body({ totalCount: 0, searchedCount: 0, visibleCount: 0 }))).toBe("zero");
  });

  it("shows rows when the narrowing keeps something", () => {
    expect(resolveArchivedModulesBody(body({ visibleCount: 2 }))).toBe("rows");
  });

  it("blames the search when it alone excluded every row", () => {
    expect(resolveArchivedModulesBody(body({ searchedCount: 0, visibleCount: 0 }))).toBe("no-match-search");
  });

  it("blames the filters when the search kept rows and the narrowing did not", () => {
    expect(resolveArchivedModulesBody(body({ searchedCount: 2, visibleCount: 0 }))).toBe("no-match-filters");
  });
});

describe("archived narrowing", () => {
  const full: ModuleListFilters = {
    favorites: true,
    status: ["completed"],
    lead: ["u1"],
    members: ["u2"],
    "start-date": [{ kind: "preset", value: "week-1" }],
    "due-date": [{ kind: "preset", value: "month-1" }],
  };

  it("offers only lead, members and the two dates", () => {
    expect([...ARCHIVED_MODULE_FILTER_DIMENSIONS]).toEqual(["lead", "members", "start-date", "due-date"]);
  });

  it("drops favorites and status, which describe live modules only", () => {
    const archived = archivedModuleFilters(full);
    expect(archived.favorites).toBe(false);
    expect(archived.status).toEqual([]);
    expect(archived.lead).toEqual(["u1"]);
    expect(archived.members).toEqual(["u2"]);
    expect(archived["start-date"]).toEqual([{ kind: "preset", value: "week-1" }]);
    expect(archived["due-date"]).toEqual([{ kind: "preset", value: "month-1" }]);
  });

  it("copies the arrays so a later edit never reaches the source", () => {
    const archived = archivedModuleFilters(full);
    archived.lead.push("u9");
    expect(full.lead).toEqual(["u1"]);
  });

  it("reads an empty narrowing as no filters at all", () => {
    expect(hasArchivedModuleFilters(emptyModuleFilters())).toBe(false);
    expect(hasArchivedModuleFilters({ ...emptyModuleFilters(), favorites: true })).toBe(false);
    expect(hasArchivedModuleFilters({ ...emptyModuleFilters(), status: ["completed"] })).toBe(false);
    expect(hasArchivedModuleFilters(full)).toBe(true);
  });
});

describe("archived row narrowing", () => {
  const rows = [
    makeModuleRow({ id: "a", name: "Atlas", lead_id: "u1", status: "completed" }),
    makeModuleRow({ id: "b", name: "Beacon", lead_id: null, status: "cancelled" }),
  ];

  it("narrows by name substring", () => {
    expect(archivedModulesSearched(rows, "atl").map((row) => row.name)).toEqual(["Atlas"]);
    expect(archivedModulesSearched(rows, "  ")).toHaveLength(2);
  });

  it("applies search and the archived narrowing together", () => {
    expect(archivedModulesVisible(rows, "a", { ...emptyModuleFilters(), lead: ["u1"] }).map((row) => row.name)).toEqual(
      ["Atlas"]
    );
    expect(archivedModulesVisible(rows, "", { ...emptyModuleFilters(), lead: ["u1"] }).map((row) => row.name)).toEqual([
      "Atlas",
    ]);
    expect(archivedModulesVisible(rows, "", { ...emptyModuleFilters(), status: ["cancelled"] })).toHaveLength(2);
  });
});

describe("toArchivedModuleRow", () => {
  function archivedRow(overrides: Partial<ArchivedModuleListItem> = {}): ArchivedModuleListItem {
    return {
      id: MODULES_MODULE_ID,
      workspace_id: "123e4567-e89b-12d3-a456-426614174000",
      project_id: MODULES_PROJECT_ID,
      name: "Archived Atlas",
      description: "Done",
      description_text: null,
      description_html: null,
      start_date: "2026-01-01",
      target_date: "2026-01-31",
      status: "completed",
      lead_id: null,
      member_ids: ["423e4567-e89b-12d3-a456-426614174003"],
      sort_order: 1024,
      total_issues: 4,
      is_favorite: false,
      cancelled_issues: 0,
      completed_issues: 3,
      started_issues: 1,
      unstarted_issues: 0,
      backlog_issues: 0,
      created_at: "2026-01-02T03:04:05Z",
      updated_at: "2026-02-03T04:05:06Z",
      archived_at: "2026-03-01T00:00:00Z",
      ...overrides,
    };
  }

  it("copies the fields the shared row reads", () => {
    const row = toArchivedModuleRow(archivedRow());
    expect(row).toMatchObject({
      id: MODULES_MODULE_ID,
      name: "Archived Atlas",
      status: "completed",
      sort_order: 1024,
      total_issues: 4,
      completed_issues: 3,
      archived_at: "2026-03-01T00:00:00Z",
    });
    expect(row.member_ids).toEqual(["423e4567-e89b-12d3-a456-426614174003"]);
  });

  it("keeps the member list independent of the source record", () => {
    const source = archivedRow();
    const row = toArchivedModuleRow(source);
    row.member_ids.push("523e4567-e89b-12d3-a456-426614174004");
    expect(source.member_ids).toHaveLength(1);
  });

  it("reads an unknown archived status as done rather than dropping the row", () => {
    expect(toArchivedModuleRow(archivedRow({ status: "mystery" })).status).toBe("completed");
  });
});
