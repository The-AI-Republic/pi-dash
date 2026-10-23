// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cycles area barrel (NEWFRONT-859 seeds it with the member-timezone
// date display; slice A extends it with the route shell and states;
// slice B adds the list body, rows, search, peek, and share helpers;
// slice C adds the list filters, menu, chips, and predicate).
export { cycleDetailKeys, cycleDetailQueryOptions, useCycleDetail } from "./api/cycleDetail.js";
export {
  resetCycleFilterStore,
  useCycleFilterStore,
  useCycleListFilter,
  type CycleListFilter,
} from "./api/filterStore.js";
export {
  invalidateAfterCycleTransfer,
  invalidateAfterCycleWrite,
  invalidateArchivedCycleReads,
  invalidateCycleLists,
  invalidateCycleReads,
} from "./api/invalidate.js";
export { cycleKeys } from "./api/keys.js";
export { archivedCyclesQueryOptions, cyclesListQueryOptions } from "./api/queries.js";
export { resetCycleSearchStore, useCycleSearchStore } from "./api/searchStore.js";
export {
  CYCLE_MEMBER_DATES_LABEL,
  CycleMemberDates,
  CycleMemberDatesFrame,
  type CycleMemberDateRange,
} from "./components/CycleMemberDates.js";
export { CycleDateSpan, type CycleDateSpanProps } from "./components/CycleDateSpan.js";
export { CycleDetailGate, type CycleDetailGateProps } from "./components/CycleDetailGate.js";
export { CycleFilterChips, type CycleFilterChipsProps } from "./components/CycleFilterChips.js";
export {
  CYCLE_FILTER_DIMENSIONS,
  CycleFilterMenu,
  focusFirstCalendarDay,
  type CycleFilterMenuProps,
} from "./components/CycleFilterMenu.js";
export { CycleGoneAway, type CycleGoneAwayProps } from "./components/CycleGoneAway.js";
export { CycleGroup, type CycleGroupProps } from "./components/CycleGroup.js";
export { CyclePeekPanel, type CyclePeekPanelProps } from "./components/CyclePeekPanel.js";
export { CycleProgressRing, type CycleProgressRingProps } from "./components/CycleProgressRing.js";
export { CycleRow, type CycleRowProps } from "./components/CycleRow.js";
export { CyclesFeatureGate, type CyclesFeatureGateProps } from "./components/CyclesFeatureGate.js";
export { CyclesListBody, type CyclesListBodyProps } from "./components/CyclesListBody.js";
export { CyclesListHeader, type CyclesListHeaderProps } from "./components/CyclesListHeader.js";
export { CyclesListScreen, type CyclesListScreenProps } from "./components/CyclesListScreen.js";
export { CyclesListSkeleton } from "./components/CyclesListSkeleton.js";
export { CyclesLoadError, type CyclesLoadErrorProps } from "./components/CyclesLoadError.js";
export { CyclesNoMatch, type CyclesNoMatchProps, type CyclesNoMatchVariant } from "./components/CyclesNoMatch.js";
export { CyclesSearchBox, type CyclesSearchBoxProps } from "./components/CyclesSearchBox.js";
export { CyclesZeroState, type CyclesZeroStateProps } from "./components/CyclesZeroState.js";
export {
  CYCLE_LIST_TAB_KEY,
  CreateCycleDialog,
  UpdateCycleDialog,
  type CreateCycleDialogProps,
  type CycleProjectRef,
  type UpdateCycleDialogProps,
} from "./dialog/CycleDialog.js";
export {
  CYCLE_OVERLAP_TITLE,
  cycleDaysComplete,
  cycleDaysUnchanged,
  cycleFailureMessage,
  cycleFormDays,
  cycleOverlapMessage,
  needsCycleDateCheck,
  validateCycleRange,
  type CyclePayloadDays,
  type CycleStoredDays,
} from "./dialog/cycleRange.js";
export { CYCLE_TITLE_MAX_LENGTH, validateCycleTitle } from "./dialog/cycleTitle.js";
export {
  cycleCoversToday,
  cycleSpanModel,
  formatCycleSpanText,
  formatOffsetChip,
  formatZoneDay,
  instantToProjectDate,
  instantZoneDay,
  projectOffsetChip,
  projectToday,
  zoneOffsetMinutes,
  type CycleSpanModel,
  type ZoneDay,
} from "./lib/cycleDates.js";
export {
  CYCLE_GROUP_HEADINGS,
  groupOfCycle,
  lifecycleLabel,
  lifecycleTone,
  type CycleListGroup,
} from "./lib/cycleGroups.js";
export {
  countCycleFilters,
  CYCLE_FILTER_CHIP_LABELS,
  CYCLE_FILTER_SECTION_TITLES,
  CYCLE_FILTER_STATUS_VALUES,
  EMPTY_CYCLE_FILTERS,
  hasCycleFilters,
  makeCycleFilter,
  parseCycleDateEntry,
  type CycleDateEntry,
  type CycleDateFilterDimension,
  type CycleFilterDimension,
  type CycleListFilters,
} from "./lib/cycleFilters.js";
export { parseCyclesListSearch } from "./lib/cycleListSearch.js";
export { selectCycleMemberDirectory, UNKNOWN_CYCLE_MEMBER, type CycleMemberEntry } from "./lib/cycleMembers.js";
export {
  cycleProgressCounts,
  cycleProgressPercent,
  type CycleProgressCounts,
  type CycleProgressInput,
} from "./lib/cycleProgress.js";
export { filterCyclesByQuery, matchesCycleQuery } from "./lib/cycleSearch.js";
export {
  CYCLES_CRUMB,
  cycleDetailPath,
  cyclesFeatureHref,
  cyclesListHref,
  cyclesTitle,
  isCycleGoneAway,
  resolveCyclesBody,
  type CyclesBodyInput,
  type CyclesBodyState,
} from "./lib/cycleScreen.js";
export {
  CYCLE_LINK_COPIED_TITLE,
  CYCLE_LINK_COPY_FAILED_TITLE,
  CYCLE_OPEN_NEW_TAB_FAILED_TITLE,
  copyCycleLink,
  openCycleInNewTab,
} from "./lib/cycleShare.js";
export { formatMemberDate } from "./lib/memberDates.js";
