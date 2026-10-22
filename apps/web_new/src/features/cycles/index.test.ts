// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  CYCLE_MEMBER_DATES_LABEL,
  cycleDetailKeys,
  cycleDetailQueryOptions,
  CycleMemberDates,
  CycleMemberDatesFrame,
  formatMemberDate,
  useCycleDetail,
} from "./index.js";

describe("cycles barrel", () => {
  it("re-exports the member-timezone date display", () => {
    expect(cycleDetailKeys).toBeDefined();
    expect(cycleDetailQueryOptions).toBeDefined();
    expect(useCycleDetail).toBeDefined();
    expect(CycleMemberDates).toBeDefined();
    expect(CycleMemberDatesFrame).toBeDefined();
    expect(CYCLE_MEMBER_DATES_LABEL).toBe("Dates in your timezone");
    expect(formatMemberDate).toBeDefined();
  });
});
