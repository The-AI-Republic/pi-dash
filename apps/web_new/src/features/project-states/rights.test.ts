// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { canEditStates, canToggleMemberEditing } from "./rights.js";

describe("canEditStates", () => {
  it("lets admins edit regardless of the flag", () => {
    expect(canEditStates(20, false)).toBe(true);
    expect(canEditStates(20, true)).toBe(true);
  });

  it("lets members edit only while the flag is on", () => {
    expect(canEditStates(15, true)).toBe(true);
    expect(canEditStates(15, false)).toBe(false);
  });

  it("never lets guests or outsiders edit", () => {
    expect(canEditStates(5, true)).toBe(false);
    expect(canEditStates(null, true)).toBe(false);
    expect(canEditStates(0, true)).toBe(false);
  });
});

describe("canToggleMemberEditing", () => {
  it("is admin-only", () => {
    expect(canToggleMemberEditing(20)).toBe(true);
    expect(canToggleMemberEditing(15)).toBe(false);
    expect(canToggleMemberEditing(5)).toBe(false);
    expect(canToggleMemberEditing(null)).toBe(false);
  });
});
