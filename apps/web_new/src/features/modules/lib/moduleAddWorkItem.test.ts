// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module add standing (parity/specs/modules.md §8.3, MOD-031): a live
// project's admin or member gets the header and empty-card entries; every
// guest standing and the archived project read false. The module's own
// status plays no part (the old header gated only on the project role).
// The list row also maps to the shared dialog's option here, since its
// field names differ.

import { describe, expect, it } from "vitest";

import {
  MODULE_ADD_EXISTING_FAILURE,
  MODULE_ADD_EXISTING_SUCCESS,
  MODULE_ADD_WORK_ITEM_LABEL,
  canAddWorkItemsToModule,
  toModuleCreateProject,
} from "./moduleAddWorkItem.js";

describe("module add copy", () => {
  it("pins the header label and both notices", () => {
    expect(MODULE_ADD_WORK_ITEM_LABEL).toBe("Add work item");
    expect(MODULE_ADD_EXISTING_SUCCESS).toBe("Work items added to the module successfully.");
    expect(MODULE_ADD_EXISTING_FAILURE).toBe("Selected work items could not be added to the module.");
  });
});

describe("canAddWorkItemsToModule", () => {
  it("lets a project admin and a project member add to a live project", () => {
    expect(canAddWorkItemsToModule({ memberRole: 20, archived: false })).toBe(true);
    expect(canAddWorkItemsToModule({ memberRole: 15, archived: false })).toBe(true);
  });

  it("refuses a project guest and a viewer with no role", () => {
    expect(canAddWorkItemsToModule({ memberRole: 5, archived: false })).toBe(false);
    expect(canAddWorkItemsToModule({ memberRole: null, archived: false })).toBe(false);
  });

  it("refuses an archived project even for its admin", () => {
    expect(canAddWorkItemsToModule({ memberRole: 20, archived: true })).toBe(false);
  });
});

describe("toModuleCreateProject", () => {
  const row = {
    id: "223e4567-e89b-12d3-a456-426614174001",
    name: "Website",
    identifier: "WEB",
    member_role: 15,
    archived_at: null,
  };

  it("renames the API row's role and archive fields for the dialog", () => {
    expect(toModuleCreateProject(row)).toEqual({
      id: row.id,
      name: "Website",
      identifier: "WEB",
      memberRole: 15,
      archived: false,
    });
  });

  it("keeps a guest role null-ish and an archived project flagged", () => {
    expect(toModuleCreateProject({ ...row, member_role: null }).memberRole).toBe(null);
    expect(toModuleCreateProject({ ...row, archived_at: "2026-01-02T03:04:05Z" }).archived).toBe(true);
  });
});
