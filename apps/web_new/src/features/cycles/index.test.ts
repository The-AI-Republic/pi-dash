// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  archivedCyclesQueryOptions,
  CYCLE_MEMBER_DATES_LABEL,
  CYCLES_CRUMB,
  cycleCoversToday,
  cycleDetailKeys,
  cycleDetailQueryOptions,
  CycleDateSpan,
  CycleDetailGate,
  CycleGoneAway,
  CycleMemberDates,
  CycleMemberDatesFrame,
  cycleKeys,
  cycleSpanModel,
  CyclesFeatureGate,
  cyclesFeatureHref,
  cyclesListHref,
  cyclesListQueryOptions,
  CyclesListScreen,
  CyclesListSkeleton,
  CyclesLoadError,
  cyclesTitle,
  CyclesZeroState,
  formatCycleSpanText,
  formatMemberDate,
  formatOffsetChip,
  formatZoneDay,
  instantToProjectDate,
  instantZoneDay,
  invalidateAfterCycleTransfer,
  invalidateAfterCycleWrite,
  invalidateArchivedCycleReads,
  invalidateCycleLists,
  invalidateCycleReads,
  isCycleGoneAway,
  projectOffsetChip,
  projectToday,
  resolveCyclesBody,
  useCycleDetail,
  zoneOffsetMinutes,
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

  it("re-exports the route shell and its states", () => {
    expect(cycleKeys).toBeDefined();
    expect(cyclesListQueryOptions).toBeDefined();
    expect(archivedCyclesQueryOptions).toBeDefined();
    expect(invalidateCycleLists).toBeDefined();
    expect(invalidateCycleReads).toBeDefined();
    expect(invalidateArchivedCycleReads).toBeDefined();
    expect(invalidateAfterCycleWrite).toBeDefined();
    expect(invalidateAfterCycleTransfer).toBeDefined();
    expect(CYCLES_CRUMB).toBe("Cycles");
    expect(cyclesTitle).toBeDefined();
    expect(cyclesListHref).toBeDefined();
    expect(cyclesFeatureHref).toBeDefined();
    expect(resolveCyclesBody).toBeDefined();
    expect(isCycleGoneAway).toBeDefined();
    expect(CyclesListScreen).toBeDefined();
    expect(CyclesFeatureGate).toBeDefined();
    expect(CyclesZeroState).toBeDefined();
    expect(CyclesListSkeleton).toBeDefined();
    expect(CyclesLoadError).toBeDefined();
    expect(CycleGoneAway).toBeDefined();
    expect(CycleDetailGate).toBeDefined();
  });

  it("re-exports the shared date helper", () => {
    expect(CycleDateSpan).toBeDefined();
    expect(cycleSpanModel).toBeDefined();
    expect(formatCycleSpanText).toBeDefined();
    expect(formatZoneDay).toBeDefined();
    expect(formatOffsetChip).toBeDefined();
    expect(projectOffsetChip).toBeDefined();
    expect(zoneOffsetMinutes).toBeDefined();
    expect(instantZoneDay).toBeDefined();
    expect(instantToProjectDate).toBeDefined();
    expect(projectToday).toBeDefined();
    expect(cycleCoversToday).toBeDefined();
  });
});
