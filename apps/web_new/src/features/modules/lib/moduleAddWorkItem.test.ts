// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module add standing (parity/specs/modules.md §8.3, MOD-031): only a
// live project's admin or member on a module that is not complete gets the
// header and empty-card entries; every guest standing and the archived
// project read false.

import { describe, expect, it } from "vitest";

import {
  MODULE_ADD_EXISTING_FAILURE,
  MODULE_ADD_EXISTING_SUCCESS,
  MODULE_ADD_WORK_ITEM_LABEL,
  canAddWorkItemsToModule,
} from "./moduleAddWorkItem.js";

describe("module add copy", () => {
  it("pins the header label and both notices", () => {
    expect(MODULE_ADD_WORK_ITEM_LABEL).toBe("Add work item");
    expect(MODULE_ADD_EXISTING_SUCCESS).toBe("Work items added to the module successfully.");
    expect(MODULE_ADD_EXISTING_FAILURE).toBe("Selected work items could not be added to the module.");
  });
});

describe("canAddWorkItemsToModule", () => {
  it("lets a project admin and a project member add to a live module", () => {
    expect(canAddWorkItemsToModule({ memberRole: 20, archived: false, moduleStatus: "in-progress" })).toBe(true);
    expect(canAddWorkItemsToModule({ memberRole: 15, archived: false, moduleStatus: "planned" })).toBe(true);
    expect(canAddWorkItemsToModule({ memberRole: 15, archived: false, moduleStatus: undefined })).toBe(true);
  });

  it("refuses a project guest and a viewer with no role", () => {
    expect(canAddWorkItemsToModule({ memberRole: 5, archived: false, moduleStatus: "in-progress" })).toBe(false);
    expect(canAddWorkItemsToModule({ memberRole: null, archived: false, moduleStatus: "in-progress" })).toBe(false);
  });

  it("treats a completed module as a record", () => {
    expect(canAddWorkItemsToModule({ memberRole: 20, archived: false, moduleStatus: "completed" })).toBe(false);
    expect(canAddWorkItemsToModule({ memberRole: 20, archived: false, moduleStatus: "COMPLETED" })).toBe(false);
  });

  it("refuses an archived project even for its admin", () => {
    expect(canAddWorkItemsToModule({ memberRole: 20, archived: true, moduleStatus: "in-progress" })).toBe(false);
  });
});
