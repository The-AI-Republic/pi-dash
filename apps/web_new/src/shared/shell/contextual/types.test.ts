// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { canEditInProject, CONTEXT_INDICATORS, CONTEXT_SECTIONS, workItemNoun } from "./types.js";

describe("contextual types", () => {
  it("names every family section and chip indicator", () => {
    expect(CONTEXT_SECTIONS).toEqual({
      "work-item": "Work item actions",
      cycle: "Cycle actions",
      module: "Module actions",
      page: "Page actions",
    });
    expect(CONTEXT_INDICATORS).toEqual({
      "work-item": "Work item",
      cycle: "Cycle",
      module: "Module",
      page: "Page",
    });
  });

  it("gates edits on project admin or member standing", () => {
    expect(canEditInProject({ memberRole: 20, archived: false })).toBe(true);
    expect(canEditInProject({ memberRole: 15, archived: false })).toBe(true);
    expect(canEditInProject({ memberRole: 5, archived: false })).toBe(false);
    expect(canEditInProject({ memberRole: null, archived: false })).toBe(false);
    expect(canEditInProject({ memberRole: 20, archived: true })).toBe(false);
    expect(canEditInProject({ memberRole: 15, archived: true })).toBe(false);
  });

  it("names epics and sub-items for toasts", () => {
    expect(workItemNoun({ isEpic: true, parentId: null })).toBe("Epic");
    expect(workItemNoun({ isEpic: false, parentId: "parent" })).toBe("Sub-work item");
    expect(workItemNoun({ isEpic: false, parentId: null })).toBe("Work item");
  });
});
