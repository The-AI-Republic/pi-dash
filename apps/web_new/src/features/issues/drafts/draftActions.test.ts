// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row-action catalog (parity/specs/drafts.md §3.1, DRAFT-009): pinned
// order, resolved labels, and internal ids that never render.

import { describe, expect, it } from "vitest";

import { DRAFT_ROW_ACTION_IDS, DRAFT_ROW_ACTIONS, type DraftRowActionId } from "./draftActions.js";

describe("DRAFT_ROW_ACTIONS", () => {
  it("lists the four entries in pinned order with resolved labels", () => {
    expect(DRAFT_ROW_ACTIONS.map((action) => action.label)).toEqual([
      "Edit",
      "Make a copy",
      "Move to project",
      "Delete",
    ]);
    expect(DRAFT_ROW_ACTIONS.map((action) => action.id)).toEqual([...DRAFT_ROW_ACTION_IDS]);
  });

  it("flags only the delete entry as destructive", () => {
    const flagged = DRAFT_ROW_ACTIONS.filter((action) => action.danger).map((action) => action.id);
    expect(flagged).toEqual(["delete"]);
  });

  it("never exposes a raw i18n key as a label", () => {
    const rawKeys = new Set(["edit", "make_a_copy", "move_to_project", "delete"]);
    for (const action of DRAFT_ROW_ACTIONS) {
      expect(rawKeys.has(action.label)).toBe(false);
      expect(action.label.trim()).not.toBe("");
    }
    const ids: readonly string[] = DRAFT_ROW_ACTIONS.map((action) => action.id);
    expect(new Set(ids).size).toBe(ids.length);
    const everyId: DraftRowActionId[] = ["edit", "copy", "move", "delete"];
    expect([...DRAFT_ROW_ACTION_IDS]).toEqual(everyId);
  });
});
