// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { Project } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  ALL_PROJECTS_LABEL,
  DEFAULT_EXPORT_PROVIDER,
  EXPORT_FORMAT_OPTIONS,
  exportRequestBody,
  formatLabel,
  parseExportForm,
  selectExportProjectIds,
  selectPickedIdentifiers,
  selectProjectTriggerLabel,
} from "./exportForm.js";

const WORKSPACE = "44444444-4444-4444-8444-444444444444";

function projectRow(overrides: Partial<Project> = {}): Project {
  return {
    id: "22222222-2222-4222-8222-222222222222",
    name: "Seed project",
    identifier: "SEED",
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
    ...overrides,
  };
}

describe("export form model", () => {
  it("offers the three formats in oracle order with CSV first", () => {
    expect(EXPORT_FORMAT_OPTIONS.map((option) => option.label)).toEqual(["CSV", "Excel", "JSON"]);
    expect(EXPORT_FORMAT_OPTIONS.map((option) => option.provider)).toEqual(["csv", "xlsx", "json"]);
    expect(DEFAULT_EXPORT_PROVIDER).toBe("csv");
  });

  it("labels known providers and passes unknown ids through", () => {
    expect(formatLabel("csv")).toBe("CSV");
    expect(formatLabel("xlsx")).toBe("Excel");
    expect(formatLabel("json")).toBe("JSON");
    expect(formatLabel("pdf")).toBe("pdf");
  });

  it("parses a complete form value", () => {
    expect(parseExportForm({ provider: "json", projectIds: ["a", "b"] })).toEqual({
      provider: "json",
      projectIds: ["a", "b"],
    });
  });

  it("falls back to CSV for missing and unknown providers", () => {
    expect(parseExportForm({})).toEqual({ provider: "csv", projectIds: [] });
    expect(parseExportForm({ provider: "pdf", projectIds: [] })).toEqual({ provider: "csv", projectIds: [] });
    expect(parseExportForm({ provider: 7, projectIds: [] })).toEqual({ provider: "csv", projectIds: [] });
  });

  it("keeps only string project ids and defaults non-lists to empty", () => {
    expect(parseExportForm({ provider: "csv", projectIds: ["a", 7, null] })).toEqual({
      provider: "csv",
      projectIds: ["a"],
    });
    expect(parseExportForm({ provider: "csv", projectIds: "a" })).toEqual({ provider: "csv", projectIds: [] });
  });

  it("flags several projects for separate files, never one or none", () => {
    expect(exportRequestBody({ provider: "csv", projectIds: [] })).toEqual({
      provider: "csv",
      project: [],
      multiple: false,
    });
    expect(exportRequestBody({ provider: "xlsx", projectIds: ["a"] })).toEqual({
      provider: "xlsx",
      project: ["a"],
      multiple: false,
    });
    expect(exportRequestBody({ provider: "json", projectIds: ["a", "b"] })).toEqual({
      provider: "json",
      project: ["a", "b"],
      multiple: true,
    });
  });

  it("copies the project list into the request body", () => {
    const projectIds = ["a"];
    const body = exportRequestBody({ provider: "csv", projectIds });
    expect(body.project).toEqual(["a"]);
    expect(body.project).not.toBe(projectIds);
  });

  it("offers joined unarchived member-or-admin projects only", () => {
    const rows = [
      projectRow({ id: "a", identifier: "A", member_role: 15 }),
      projectRow({ id: "b", identifier: "B", member_role: 20 }),
      projectRow({ id: "c", identifier: "C", member_role: 5 }),
      projectRow({ id: "d", identifier: "D", member_role: null }),
      projectRow({ id: "e", identifier: "E", member_role: 15, archived_at: "2026-02-01T00:00:00.000Z" }),
    ];
    expect(selectExportProjectIds(rows)).toEqual(["a", "b"]);
  });

  it("resolves picked keys in pick order and drops stale ids", () => {
    const rows = [
      { id: "a", identifier: "EA" },
      { id: "b", identifier: "EB" },
    ];
    expect(selectPickedIdentifiers(["b", "a"], rows)).toEqual(["EB", "EA"]);
    expect(selectPickedIdentifiers(["b", "gone"], rows)).toEqual(["EB"]);
    expect(selectPickedIdentifiers([], rows)).toEqual([]);
  });

  it("compacts picked keys and reads All projects when empty", () => {
    expect(ALL_PROJECTS_LABEL).toBe("All projects");
    expect(selectProjectTriggerLabel([])).toBe("All projects");
    expect(selectProjectTriggerLabel(["EA"])).toBe("EA");
    expect(selectProjectTriggerLabel(["EA", "EB"])).toBe("EA, EB");
  });
});
