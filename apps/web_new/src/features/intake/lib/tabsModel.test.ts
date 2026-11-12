// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { INTAKE_STATUS } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  INTAKE_TAB_STATUSES,
  shouldShowOpenBadge,
  siblingTab,
  statusParamForTab,
  statusSetForTab,
  tabIndex,
} from "./tabsModel.js";

describe("intake tab defaults (INT-012)", () => {
  it("reads Pending only on Open", () => {
    expect(statusSetForTab("open")).toEqual([INTAKE_STATUS.pending]);
    expect(statusParamForTab("open")).toBe("-2");
  });

  it("reads accepted, declined and duplicate on Closed, in that wire order", () => {
    expect([...INTAKE_TAB_STATUSES.closed]).toEqual([
      INTAKE_STATUS.accepted,
      INTAKE_STATUS.declined,
      INTAKE_STATUS.duplicate,
    ]);
    expect(statusParamForTab("closed")).toBe("1,-1,2");
  });

  it("never includes Snoozed in a default set", () => {
    expect(statusSetForTab("open")).not.toContain(INTAKE_STATUS.snoozed);
    expect(statusSetForTab("closed")).not.toContain(INTAKE_STATUS.snoozed);
  });
});

describe("badge and ordering helpers", () => {
  it("shows the Open badge only while Open is active (INT-004)", () => {
    expect(shouldShowOpenBadge("open")).toBe(true);
    expect(shouldShowOpenBadge("closed")).toBe(false);
  });

  it("siblings and indexes follow display order", () => {
    expect(siblingTab("open")).toBe("closed");
    expect(siblingTab("closed")).toBe("open");
    expect(tabIndex("open")).toBe(0);
    expect(tabIndex("closed")).toBe(1);
  });
});
