// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline edit standing (ISS-050–055/058).
import { describe, expect, it } from "vitest";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { canEditTimeline, type TimelineStanding } from "./permissions.js";

function standing(overrides: Partial<TimelineStanding> = {}): TimelineStanding {
  return { projectRole: ROLE_GUEST, archived: false, ...overrides };
}

describe("canEditTimeline", () => {
  it("allows project members and admins", () => {
    expect(canEditTimeline(standing({ projectRole: ROLE_MEMBER }))).toBe(true);
    expect(canEditTimeline(standing({ projectRole: ROLE_ADMIN }))).toBe(true);
  });

  it("denies guests and non-members", () => {
    expect(canEditTimeline(standing())).toBe(false);
    expect(canEditTimeline(standing({ projectRole: null }))).toBe(false);
  });

  it("denies a project member whose role is below member", () => {
    expect(canEditTimeline(standing({ projectRole: ROLE_GUEST }))).toBe(false);
  });

  it("denies everything on an archived project", () => {
    expect(canEditTimeline(standing({ projectRole: ROLE_ADMIN, archived: true }))).toBe(false);
  });
});
