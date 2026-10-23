// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  archivedCyclesQueryOptions,
  CYCLE_GROUP_HEADINGS,
  CYCLE_LINK_COPIED_TITLE,
  CYCLE_LINK_COPY_FAILED_TITLE,
  CYCLE_LIST_TAB_KEY,
  CYCLE_MEMBER_DATES_LABEL,
  CYCLE_OPEN_NEW_TAB_FAILED_TITLE,
  CYCLE_OVERLAP_TITLE,
  CYCLE_TITLE_MAX_LENGTH,
  CYCLES_CRUMB,
  copyCycleLink,
  CreateCycleDialog,
  UpdateCycleDialog,
  cycleCoversToday,
  cycleDaysComplete,
  cycleDaysUnchanged,
  cycleDetailKeys,
  cycleDetailPath,
  cycleDetailQueryOptions,
  cycleFailureMessage,
  cycleFormDays,
  CycleDateSpan,
  CycleDetailGate,
  CycleGoneAway,
  CycleGroup,
  CycleMemberDates,
  CycleMemberDatesFrame,
  cycleKeys,
  cycleOverlapMessage,
  cycleProgressCounts,
  cycleProgressPercent,
  CyclePeekPanel,
  CycleProgressRing,
  CycleRow,
  cycleSpanModel,
  CyclesFeatureGate,
  cyclesFeatureHref,
  CyclesListBody,
  cyclesListHref,
  cyclesListQueryOptions,
  CyclesListHeader,
  CyclesListScreen,
  needsCycleDateCheck,
  validateCycleRange,
  validateCycleTitle,
  CyclesListSkeleton,
  CyclesLoadError,
  CyclesNoMatch,
  CyclesSearchBox,
  cyclesTitle,
  CyclesZeroState,
  filterCyclesByQuery,
  formatCycleSpanText,
  formatMemberDate,
  formatOffsetChip,
  formatZoneDay,
  groupOfCycle,
  instantToProjectDate,
  instantZoneDay,
  invalidateAfterCycleTransfer,
  invalidateAfterCycleWrite,
  invalidateArchivedCycleReads,
  invalidateCycleLists,
  invalidateCycleReads,
  isCycleGoneAway,
  lifecycleLabel,
  lifecycleTone,
  matchesCycleQuery,
  openCycleInNewTab,
  parseCyclesListSearch,
  projectOffsetChip,
  projectToday,
  resetCycleSearchStore,
  resolveCyclesBody,
  selectCycleMemberDirectory,
  UNKNOWN_CYCLE_MEMBER,
  useCycleDetail,
  useCycleSearchStore,
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

  it("re-exports the list body, rows, search, peek, and share helpers", () => {
    expect(CYCLE_GROUP_HEADINGS).toBeDefined();
    expect(groupOfCycle).toBeDefined();
    expect(lifecycleLabel).toBeDefined();
    expect(lifecycleTone).toBeDefined();
    expect(parseCyclesListSearch).toBeDefined();
    expect(selectCycleMemberDirectory).toBeDefined();
    expect(UNKNOWN_CYCLE_MEMBER).toBeDefined();
    expect(cycleProgressCounts).toBeDefined();
    expect(cycleProgressPercent).toBeDefined();
    expect(filterCyclesByQuery).toBeDefined();
    expect(matchesCycleQuery).toBeDefined();
    expect(cycleDetailPath).toBeDefined();
    expect(CYCLE_LINK_COPIED_TITLE).toBe("Cycle link copied");
    expect(CYCLE_LINK_COPY_FAILED_TITLE).toBe("Couldn't copy the cycle link");
    expect(CYCLE_OPEN_NEW_TAB_FAILED_TITLE).toBe("Couldn't open the cycle in a new tab");
    expect(copyCycleLink).toBeDefined();
    expect(openCycleInNewTab).toBeDefined();
    expect(useCycleSearchStore).toBeDefined();
    expect(resetCycleSearchStore).toBeDefined();
    expect(CycleGroup).toBeDefined();
    expect(CyclePeekPanel).toBeDefined();
    expect(CycleProgressRing).toBeDefined();
    expect(CycleRow).toBeDefined();
    expect(CyclesListBody).toBeDefined();
    expect(CyclesNoMatch).toBeDefined();
    expect(CyclesSearchBox).toBeDefined();
  });

  it("re-exports the save dialog, its header and its validation", () => {
    expect(CreateCycleDialog).toBeDefined();
    expect(UpdateCycleDialog).toBeDefined();
    expect(CYCLE_LIST_TAB_KEY).toBe("cycle_tab");
    expect(CyclesListHeader).toBeDefined();
    expect(CYCLE_TITLE_MAX_LENGTH).toBe(255);
    expect(validateCycleTitle).toBeDefined();
    expect(CYCLE_OVERLAP_TITLE).toBe("Error!");
    expect(cycleFormDays).toBeDefined();
    expect(cycleDaysComplete).toBeDefined();
    expect(cycleDaysUnchanged).toBeDefined();
    expect(needsCycleDateCheck).toBeDefined();
    expect(validateCycleRange).toBeDefined();
    expect(cycleOverlapMessage).toBeDefined();
    expect(cycleFailureMessage).toBeDefined();
  });
});
