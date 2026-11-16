// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import {
  ACTIVES_BENEFITS,
  ACTIVES_PRICING_HREF,
  ACTIVES_SNAPSHOTS_HEADING,
  ACTIVES_UPGRADE_LABEL,
  ActiveCycleHero,
  archivedCyclesQueryOptions,
  countCycleFilters,
  BREAKDOWN_FILTER_STEMS,
  breakdownEntryActive,
  breakdownTabKey,
  completionChartPoints,
  CYCLE_ARCHIVE_FAILED_LINE,
  CYCLE_ARCHIVE_FAILED_TITLE,
  CYCLE_ARCHIVE_GATE_HINT,
  CYCLE_ARCHIVE_LINE,
  CYCLE_ARCHIVE_TITLE,
  CYCLE_BREAKDOWN_TABS,
  CYCLE_DATES_EMPTY_LABEL,
  CYCLE_DATES_UPDATED_TITLE,
  CYCLE_DELETED_TITLE,
  CYCLE_DELETE_FAILED_LINE,
  CYCLE_DELETE_FAILED_TITLE,
  CYCLE_PROGRESS_EMPTY_TEXT,
  CYCLE_PROGRESS_MEASURES,
  CYCLE_SIDEBAR_COLLAPSE_LABEL,
  CYCLE_SIDEBAR_COLLAPSED_KEY,
  CYCLE_SIDEBAR_EXPAND_LABEL,
  CYCLE_FILTER_CHIP_LABELS,
  CYCLE_FILTER_DIMENSIONS,
  CYCLE_FILTER_SECTION_TITLES,
  CYCLE_FILTER_STATUS_VALUES,
  CYCLE_FAVORITE_ADDED_TITLE,
  CYCLE_FAVORITE_FAILED_TITLE,
  CYCLE_FAVORITE_REMOVED_TITLE,
  CYCLE_GROUP_HEADINGS,
  CYCLE_LINK_COPIED_TITLE,
  CYCLE_LINK_COPY_FAILED_TITLE,
  CYCLE_LIST_LAYOUT_OPTIONS,
  CYCLE_LIST_TAB_KEY,
  CYCLE_MEMBER_DATES_LABEL,
  CYCLE_OPEN_NEW_TAB_FAILED_TITLE,
  CYCLE_OVERLAP_TITLE,
  CYCLE_PERMISSION_TITLE,
  CYCLE_RESTORED_LINE,
  CYCLE_RESTORED_TITLE,
  CYCLE_RESTORE_FAILED_LINE,
  CYCLE_RESTORE_FAILED_TITLE,
  CYCLE_TITLE_MAX_LENGTH,
  CYCLE_TRANSFER_FAILED_TITLE,
  CYCLE_TRANSFER_NO_MATCH,
  CYCLE_TRANSFER_NO_TARGET,
  CYCLE_TRANSFER_SEARCH_PLACEHOLDER,
  CYCLE_TRANSFER_TITLE,
  CYCLE_TRANSFERRED_LINE,
  CYCLE_TRANSFERRED_TITLE,
  CYCLES_CRUMB,
  copyCycleLink,
  CreateCycleDialog,
  UpdateCycleDialog,
  cycleAnalyticsQueryOptions,
  cycleCoversToday,
  cycleDaysComplete,
  cycleDaysUnchanged,
  cycleDetailKeys,
  cycleDetailPath,
  cycleDetailQueryOptions,
  cycleFailureMessage,
  cycleFormDays,
  cycleMenuEntries,
  cycleTransferListsKey,
  flipCycleFavorite,
  flipCycleFavoriteInList,
  canTransferFromBanner,
  canTransferFromRow,
  isTransferableStatus,
  isTransferArchived,
  isTransferSnapshotEmpty,
  transferableWorkCount,
  transferStatusPill,
  transferTargetOptions,
  CycleArchiveDialog,
  CycleDateSpan,
  CycleDeleteDialog,
  CycleDetailCrumbs,
  CycleDetailGate,
  CycleDetailSidebar,
  CycleFilterChips,
  CycleFilterMenu,
  CycleFavoriteStar,
  CycleGoneAway,
  CyclePaletteEntries,
  CycleGroup,
  CycleListLayoutMenu,
  CycleMemberDates,
  CycleMemberDatesFrame,
  cycleKeys,
  cycleOverlapMessage,
  cycleProgressChartVisible,
  cycleProgressCounts,
  cycleProgressEmptyText,
  cycleProgressPercent,
  cycleProgressQueryOptions,
  CyclePeekPanel,
  CycleProgressRing,
  CycleRow,
  CycleRowMenu,
  CycleSidebarDates,
  CycleSidebarProgress,
  CycleSidebarSkeleton,
  CycleSidebarToggle,
  CycleTransferAction,
  CycleTransferDialog,
  cycleSpanModel,
  DEFAULT_BREAKDOWN_TAB,
  DEFAULT_CYCLE_LIST_LAYOUT,
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
  EMPTY_CYCLE_FILTERS,
  filterCyclesByQuery,
  FinishedCycleNotice,
  focusFirstCalendarDay,
  formatCycleSpanText,
  formatMemberDate,
  formatOffsetChip,
  formatZoneDay,
  groupOfCycle,
  hasCycleFilters,
  HeroBreakdownCard,
  HeroBurndownCard,
  HeroProgressCard,
  instantToProjectDate,
  instantZoneDay,
  invalidateAfterCycleTransfer,
  invalidateAfterCycleWrite,
  invalidateArchivedCycleReads,
  invalidateCycleLists,
  invalidateCycleReads,
  isCycleArchived,
  isCycleFinished,
  isCycleGoneAway,
  lifecycleLabel,
  lifecycleTone,
  makeCycleFilter,
  matchesCycleQuery,
  measureLabel,
  NoActiveCycle,
  openCycleInNewTab,
  parseCycleDateEntry,
  parseCycleListLayout,
  parseCyclesListSearch,
  pointsPairText,
  progressGrandTotal,
  progressShare,
  projectIssuesHref,
  projectOffsetChip,
  projectToday,
  readBreakdownTab,
  readSidebarCollapsed,
  resetCycleFilterStore,
  resetCycleSearchStore,
  resolveCyclesBody,
  selectAssigneeRows,
  selectCycleMemberDirectory,
  selectLabelRows,
  selectStateGroupRows,
  showPointsRow,
  switchCycleTarget,
  toggleBreakdownCondition,
  UNKNOWN_CYCLE_MEMBER,
  useArchiveCycle,
  useCycleAnalyticsTab,
  useCycleDetail,
  useCycleFavorite,
  useCycleFilterStore,
  useTransferCycleWork,
  useCycleListFilter,
  useCycleSearchStore,
  useCycleSidebarCollapsed,
  useDeleteCycle,
  useRestoreCycle,
  workItemsPairText,
  writeBreakdownTab,
  writeSidebarCollapsed,
  WorkspaceActivesUpsell,
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
    expect(CycleDetailCrumbs).toBeDefined();
    expect(projectIssuesHref).toBeDefined();
    expect(switchCycleTarget).toBeDefined();
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
    expect(CycleListLayoutMenu).toBeDefined();
    expect(CYCLE_LIST_LAYOUT_OPTIONS.map((option) => option.label)).toEqual(["List layout", "Gallery layout"]);
    expect(DEFAULT_CYCLE_LIST_LAYOUT).toBe("list");
    expect(parseCycleListLayout("gallery")).toBe("gallery");
    expect(CyclePeekPanel).toBeDefined();
    expect(CycleProgressRing).toBeDefined();
    expect(CycleRow).toBeDefined();
    expect(CyclesListBody).toBeDefined();
    expect(CyclesNoMatch).toBeDefined();
    expect(CyclesSearchBox).toBeDefined();
  });

  it("re-exports the list filters, menu, chips, and predicate", () => {
    expect(EMPTY_CYCLE_FILTERS).toEqual({ status: [], startDate: [], endDate: [] });
    expect(CYCLE_FILTER_DIMENSIONS).toEqual(["status", "startDate", "endDate"]);
    expect(CYCLE_FILTER_SECTION_TITLES.status).toBe("Status of the cycle");
    expect(CYCLE_FILTER_STATUS_VALUES).toContain("draft");
    expect(CYCLE_FILTER_CHIP_LABELS.endDate).toBe("Due date");
    expect(hasCycleFilters).toBeDefined();
    expect(countCycleFilters).toBeDefined();
    expect(parseCycleDateEntry).toBeDefined();
    expect(makeCycleFilter).toBeDefined();
    expect(useCycleFilterStore).toBeDefined();
    expect(resetCycleFilterStore).toBeDefined();
    expect(useCycleListFilter).toBeDefined();
    expect(CycleFilterMenu).toBeDefined();
    expect(CycleFilterChips).toBeDefined();
    expect(focusFirstCalendarDay).toBeDefined();
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

  it("re-exports the row menu, its dialogs and mutations, and the read-only notice", () => {
    expect(CYCLE_ARCHIVE_GATE_HINT).toBe("Only completed cycles can be archived");
    expect(cycleMenuEntries).toBeDefined();
    expect(isCycleArchived).toBeDefined();
    expect(isCycleFinished).toBeDefined();
    expect(CYCLE_DELETED_TITLE).toBe("Cycle deleted successfully.");
    expect(CYCLE_DELETE_FAILED_TITLE).toBe("Failed to delete cycle");
    expect(CYCLE_DELETE_FAILED_LINE).toBe("The cycle is still there. Try again.");
    expect(CYCLE_PERMISSION_TITLE).toBe("You don't have permission to perform this action.");
    expect(CYCLE_ARCHIVE_TITLE).toBe("Archive success");
    expect(CYCLE_ARCHIVE_LINE).toBe("Your archives can be found in project archives.");
    expect(CYCLE_ARCHIVE_FAILED_TITLE).toBe("Couldn't archive the cycle");
    expect(CYCLE_ARCHIVE_FAILED_LINE).toBe("The cycle is still in the live list. Try again.");
    expect(CYCLE_RESTORED_TITLE).toBe("Cycle restored");
    expect(CYCLE_RESTORED_LINE).toBe("The cycle has been restored.");
    expect(CYCLE_RESTORE_FAILED_TITLE).toBe("Cycle restore failed");
    expect(CYCLE_RESTORE_FAILED_LINE).toBe("The cycle is still archived. Try again.");
    expect(useDeleteCycle).toBeDefined();
    expect(useArchiveCycle).toBeDefined();
    expect(useRestoreCycle).toBeDefined();
    expect(CycleRowMenu).toBeDefined();
    expect(CycleDeleteDialog).toBeDefined();
    expect(CycleArchiveDialog).toBeDefined();
    expect(FinishedCycleNotice).toBeDefined();
  });

  it("re-exports the detail sidebar, its dates, progress, toggle, skeleton, state and queries", () => {
    expect(CycleDetailSidebar).toBeDefined();
    expect(CycleSidebarDates).toBeDefined();
    expect(CYCLE_DATES_UPDATED_TITLE).toBe("Cycle updated successfully.");
    expect(CYCLE_DATES_EMPTY_LABEL).toBe("Start date");
    expect(CycleSidebarProgress).toBeDefined();
    expect(CycleSidebarSkeleton).toBeDefined();
    expect(CycleSidebarToggle).toBeDefined();
    expect(CYCLE_SIDEBAR_COLLAPSE_LABEL).toBe("Collapse sidebar");
    expect(CYCLE_SIDEBAR_EXPAND_LABEL).toBe("Expand sidebar");
    expect(CYCLE_SIDEBAR_COLLAPSED_KEY).toBe("cycle_sidebar_collapsed");
    expect(readSidebarCollapsed).toBeDefined();
    expect(writeSidebarCollapsed).toBeDefined();
    expect(useCycleSidebarCollapsed).toBeDefined();
    expect(CYCLE_BREAKDOWN_TABS).toEqual(["States", "Assignees", "Labels"]);
    expect(DEFAULT_BREAKDOWN_TAB).toBe("Assignees");
    expect(breakdownTabKey).toBeDefined();
    expect(readBreakdownTab).toBeDefined();
    expect(writeBreakdownTab).toBeDefined();
    expect(useCycleAnalyticsTab).toBeDefined();
    expect(CYCLE_PROGRESS_MEASURES).toEqual([
      { value: "issues", label: "Work items" },
      { value: "points", label: "Estimates" },
    ]);
    expect(measureLabel).toBeDefined();
    expect(progressGrandTotal).toBeDefined();
    expect(progressShare).toBeDefined();
    expect(selectStateGroupRows).toBeDefined();
    expect(selectAssigneeRows).toBeDefined();
    expect(selectLabelRows).toBeDefined();
    expect(cycleProgressChartVisible).toBeDefined();
    expect(cycleProgressEmptyText).toBeDefined();
    expect(CYCLE_PROGRESS_EMPTY_TEXT).toBe("No Data yet");
    expect(completionChartPoints).toBeDefined();
    expect(workItemsPairText).toBeDefined();
    expect(pointsPairText).toBeDefined();
    expect(showPointsRow).toBeDefined();
    expect(BREAKDOWN_FILTER_STEMS).toEqual({ States: "state_group", Assignees: "assignee_id", Labels: "label_id" });
    expect(toggleBreakdownCondition).toBeDefined();
    expect(breakdownEntryActive).toBeDefined();
    expect(cycleProgressQueryOptions).toBeDefined();
    expect(cycleAnalyticsQueryOptions).toBeDefined();
  });

  it("re-exports the favorites toggle and palette mount", () => {
    expect(CYCLE_FAVORITE_ADDED_TITLE).toBe("Added to favorites");
    expect(CYCLE_FAVORITE_REMOVED_TITLE).toBe("Removed from favorites");
    expect(CYCLE_FAVORITE_FAILED_TITLE).toBe("Couldn't update favorites");
    expect(flipCycleFavorite).toBeDefined();
    expect(flipCycleFavoriteInList).toBeDefined();
    expect(useCycleFavorite).toBeDefined();
    expect(CycleFavoriteStar).toBeDefined();
    expect(CyclePaletteEntries).toBeDefined();
  });

  it("re-exports the transfer gates, write, and dialog", () => {
    expect(CYCLE_TRANSFERRED_TITLE).toBe("Success!");
    expect(CYCLE_TRANSFERRED_LINE).toBe("The remaining work items transferred successfully.");
    expect(CYCLE_TRANSFER_FAILED_TITLE).toBe("Unable to transfer work items. Please try again.");
    expect(CYCLE_TRANSFER_TITLE).toBe("Transfer work items");
    expect(CYCLE_TRANSFER_SEARCH_PLACEHOLDER).toBe("Search for a cycle...");
    expect(CYCLE_TRANSFER_NO_TARGET).toBe(
      "You don't have any current cycle. Please create one to transfer the work items."
    );
    expect(CYCLE_TRANSFER_NO_MATCH).toBe("No cycles match your search.");
    expect(canTransferFromRow).toBeDefined();
    expect(canTransferFromBanner).toBeDefined();
    expect(isTransferableStatus).toBeDefined();
    expect(isTransferArchived).toBeDefined();
    expect(isTransferSnapshotEmpty).toBeDefined();
    expect(transferableWorkCount).toBeDefined();
    expect(transferStatusPill).toBeDefined();
    expect(transferTargetOptions).toBeDefined();
    expect(cycleTransferListsKey).toBeDefined();
    expect(useTransferCycleWork).toBeDefined();
    expect(CycleTransferAction).toBeDefined();
    expect(CycleTransferDialog).toBeDefined();
  });

  it("re-exports the active-cycle hero and the workspace actives upsell", () => {
    expect(ActiveCycleHero).toBeDefined();
    expect(HeroProgressCard).toBeDefined();
    expect(HeroBurndownCard).toBeDefined();
    expect(HeroBreakdownCard).toBeDefined();
    expect(NoActiveCycle).toBeDefined();
    expect(WorkspaceActivesUpsell).toBeDefined();
    expect(ACTIVES_BENEFITS).toHaveLength(6);
    expect(ACTIVES_SNAPSHOTS_HEADING).toContain("snapshots");
    expect(ACTIVES_PRICING_HREF).toContain("pricing");
    expect(ACTIVES_UPGRADE_LABEL).toBe("Upgrade");
  });
});
