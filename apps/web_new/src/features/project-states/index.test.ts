// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The feature's public surface: the states route and later slices import
// only from this barrel, so every promised name must resolve here.
import { describe, expect, it } from "vitest";

import * as barrel from "./index.js";

describe("project states barrel", () => {
  it("exports the full editing surface and its data layer", () => {
    expect(Object.keys(barrel).length).toBeGreaterThan(40);
    for (const name of [
      "StatesTab",
      "StateGroupCard",
      "StateRow",
      "StateForm",
      "StateDeleteDialog",
      "MemberEditingToggle",
      "GroupGlyph",
      "useProjectStates",
      "useCreateState",
      "useUpdateState",
      "useMoveState",
      "useDeleteState",
      "useMarkStateDefault",
      "useSetMemberEditing",
      "groupStates",
      "resolveStateMove",
      "canEditStates",
      "canToggleMemberEditing",
      "validateStateName",
      "stateKeys",
    ]) {
      expect(barrel).toHaveProperty(name);
    }
  });

  it("keeps the seven fixed groups and the review fold", () => {
    expect(barrel.WORKFLOW_GROUPS).toHaveLength(7);
    expect(barrel.COLLAPSED_ON_LOAD).toEqual(["review"]);
    expect(barrel.STATE_ORDER_STEP).toBe(65535);
    expect(barrel.STATE_NAME_MAX_LENGTH).toBe(100);
    expect(barrel.SKELETON_ROWS).toBe(4);
  });
});
