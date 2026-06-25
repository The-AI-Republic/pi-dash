/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IIssueDisplayFilterOptions } from "@pi-dash/types";
import { EIssueLayoutTypes } from "@pi-dash/types";
import { describe, expect, it } from "vitest";

import {
  getWorkspaceDefaultDisplayFilters,
  normalizeWorkspaceDisplayFilters,
  shouldUseAllIssuesDisplayDefaults,
  WORKSPACE_ALL_ISSUES_DISPLAY_FILTERS,
} from "@/store/issue/workspace/display-filter-defaults";

describe("workspace display filter defaults", () => {
  it("defaults all work items to the project-style grouped list", () => {
    expect(getWorkspaceDefaultDisplayFilters("all-issues")).toEqual(WORKSPACE_ALL_ISSUES_DISPLAY_FILTERS);
  });

  it("keeps other static workspace views on the spreadsheet default", () => {
    expect(getWorkspaceDefaultDisplayFilters("assigned")).toEqual({
      layout: EIssueLayoutTypes.SPREADSHEET,
      order_by: "-created_at",
    });
  });

  it("migrates stale all work items filters that would render a flat list", () => {
    const flatListFilters: IIssueDisplayFilterOptions = {
      layout: EIssueLayoutTypes.LIST,
      order_by: "priority",
    };

    expect(shouldUseAllIssuesDisplayDefaults("all-issues", flatListFilters)).toBe(true);
    expect(normalizeWorkspaceDisplayFilters("all-issues", flatListFilters, flatListFilters)).toEqual({
      ...flatListFilters,
      ...WORKSPACE_ALL_ISSUES_DISPLAY_FILTERS,
    });
  });

  it("migrates the previous all work items board default", () => {
    const oldBoardFilters: IIssueDisplayFilterOptions = {
      layout: EIssueLayoutTypes.KANBAN,
      group_by: "state",
      order_by: "-created_at",
    };

    expect(shouldUseAllIssuesDisplayDefaults("all-issues", oldBoardFilters)).toBe(true);
    expect(normalizeWorkspaceDisplayFilters("all-issues", oldBoardFilters, oldBoardFilters)).toEqual({
      ...oldBoardFilters,
      ...WORKSPACE_ALL_ISSUES_DISPLAY_FILTERS,
    });
  });

  it("preserves deliberately grouped custom all work items filters", () => {
    const groupedFilters: IIssueDisplayFilterOptions = {
      layout: EIssueLayoutTypes.LIST,
      group_by: "project",
      order_by: "priority",
    };

    expect(shouldUseAllIssuesDisplayDefaults("all-issues", groupedFilters)).toBe(false);
    expect(normalizeWorkspaceDisplayFilters("all-issues", groupedFilters, groupedFilters)).toBe(groupedFilters);
  });
});
