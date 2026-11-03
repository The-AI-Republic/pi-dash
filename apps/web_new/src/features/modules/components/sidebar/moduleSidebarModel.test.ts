// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { EstimateList } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  MODULE_SAVE_SUCCESS,
  MODULE_STATUS_LABELS,
  moduleCountText,
  moduleSidebarEditable,
  showsModulePoints,
} from "./moduleSidebarModel.js";

function estimate(type: string, lastUsed = true): EstimateList[number] {
  return {
    id: "323e4567-e89b-12d3-a456-426614174002",
    workspace: "123e4567-e89b-12d3-a456-426614174000",
    project: "223e4567-e89b-12d3-a456-426614174001",
    name: "Estimate",
    description: "",
    type,
    last_used: lastUsed,
    points: [],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    created_by: null,
    updated_by: null,
  };
}

describe("moduleSidebarModel", () => {
  it("pins the status wording and success notice from the spec", () => {
    expect(MODULE_STATUS_LABELS).toEqual({
      backlog: "Backlog",
      planned: "Planned",
      "in-progress": "In Progress",
      paused: "Paused",
      completed: "Completed",
      cancelled: "Cancelled",
    });
    expect(MODULE_SAVE_SUCCESS).toBe("Module updated successfully.");
  });

  it("formats completed-over-total with missing counts as zero", () => {
    expect(moduleCountText(1, 2)).toBe("1/2");
    expect(moduleCountText(undefined, 2)).toBe("0/2");
    expect(moduleCountText(1, undefined)).toBe("1/0");
    expect(moduleCountText(undefined, undefined)).toBe("0/0");
  });

  it("allows edits only for an editor on a live, read-write mount", () => {
    expect(moduleSidebarEditable({ readOnly: false, archived: false, canEdit: true })).toBe(true);
    expect(moduleSidebarEditable({ readOnly: true, archived: false, canEdit: true })).toBe(false);
    expect(moduleSidebarEditable({ readOnly: false, archived: true, canEdit: true })).toBe(false);
    expect(moduleSidebarEditable({ readOnly: false, archived: false, canEdit: false })).toBe(false);
  });

  it("shows the points row only for an active points system", () => {
    expect(showsModulePoints([estimate("points")])).toBe(true);
    expect(showsModulePoints([estimate("time")])).toBe(false);
    expect(showsModulePoints([estimate("categories")])).toBe(false);
    expect(showsModulePoints([estimate("points", false)])).toBe(false);
    expect(showsModulePoints([])).toBe(false);
    expect(showsModulePoints(undefined)).toBe(false);
  });
});
