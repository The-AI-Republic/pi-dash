// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  LOOP_LIST_EMPTY_TEXT,
  LOOP_MIN_ROLE_LABELS,
  LOOP_TOGGLE_FAILURE_MESSAGE,
  LOOP_TOGGLE_FAILURE_TITLE,
  resolveLoopMinRoleLabel,
} from "./loopJobs.js";

describe("loop role labels", () => {
  it("maps the pinned numerics to Admin, Member and Guest", () => {
    expect(LOOP_MIN_ROLE_LABELS).toEqual({ 20: "Admin", 15: "Member", 5: "Guest" });
    expect(resolveLoopMinRoleLabel(20)).toBe("Admin");
    expect(resolveLoopMinRoleLabel(15)).toBe("Member");
    expect(resolveLoopMinRoleLabel(5)).toBe("Guest");
  });

  it("falls back to the raw value for unknown roles", () => {
    expect(resolveLoopMinRoleLabel(10)).toBe("10");
  });
});

describe("loop pinned copy", () => {
  it("keeps the oracle-pinned strings exact", () => {
    expect(LOOP_LIST_EMPTY_TEXT).toBe("No jobs yet. Create one to get started.");
    expect(LOOP_TOGGLE_FAILURE_TITLE).toBe("Update failed");
    expect(LOOP_TOGGLE_FAILURE_MESSAGE).toBe("Could not change the job state.");
  });
});
