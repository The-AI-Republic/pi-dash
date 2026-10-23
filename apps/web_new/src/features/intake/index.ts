// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the intake feature (parity/specs/intake.md). Routes import
// from here only; sibling slices (list rows, create, detail, actions) add
// their own exports as they land.

export { INTAKE_DEFAULT_ORDER, intakeKeys } from "./api/keys.js";
export type { IntakeListFilters, IntakeListKeyInput } from "./api/keys.js";
export {
  intakeDetailQueryOptions,
  intakeDetailWorkItem,
  isMissingIntakeDetail,
  useIntakeDetail,
} from "./api/detail.js";
export { paintIntakeDetailPatch, useUpdateIntakeIssue } from "./api/mutations.js";
export type { UpdateIntakeIssueScope } from "./api/mutations.js";
export {
  INTAKE_PAGE_SIZE,
  INTAKE_QUERY_POLICY,
  intakeListQueryOptions,
  intakeRows,
  intakeTotal,
  listStatuses,
  prefetchIntakeList,
  useIntakeList,
} from "./api/queries.js";
export type { IntakeListRead } from "./api/queries.js";
export { IntakeDetail } from "./components/IntakeDetail.js";
export type { IntakeDetailProps } from "./components/IntakeDetail.js";
export { IntakeDetailHeader } from "./components/IntakeDetailHeader.js";
export type { IntakeDetailHeaderProps } from "./components/IntakeDetailHeader.js";
export { IntakeEmptyState } from "./components/IntakeEmptyState.js";
export type { IntakeEmptyStateProps } from "./components/IntakeEmptyState.js";
export { IntakeProperties } from "./components/IntakeProperties.js";
export type { IntakePropertiesProps } from "./components/IntakeProperties.js";
export { IntakeGate } from "./components/IntakeGate.js";
export type { IntakeGateProps } from "./components/IntakeGate.js";
export { IntakeScreen } from "./components/IntakeScreen.js";
export type { IntakeScreenProps } from "./components/IntakeScreen.js";
export { INTAKE_LIST_PANEL_ID, IntakeTabs, openTabAccessibleName } from "./components/IntakeTabs.js";
export type { IntakeTabsProps } from "./components/IntakeTabs.js";
export { selectIntakeEmptyState } from "./lib/emptyStates.js";
export type { IntakeEmptyInput, IntakeEmptyState as IntakeEmptyKind } from "./lib/emptyStates.js";
export { intakeFeatureSettingsHref, intakeHref, retiredInboxHref } from "./lib/paths.js";
export { INTAKE_NAV_OVERLAY_SELECTOR, neighborIntakeId, suppressIntakeArrowKey } from "./lib/navigation.js";
export type { IntakeStepDirection } from "./lib/navigation.js";
export { isResolvedIntakeStatus, selectIntakeEditability } from "./lib/permissions.js";
export type { IntakeEditability, IntakeStanding } from "./lib/permissions.js";
export { parseIntakeSearch } from "./lib/search.js";
export type { IntakeSearch, IntakeSearchWriteOptions } from "./lib/search.js";
export {
  DEFAULT_INTAKE_SEARCH,
  intakeSearchAfterRemove,
  intakeSearchFor,
  settleIntakeSearch,
  withoutSelection,
} from "./lib/searchWrites.js";
export { DEFAULT_INTAKE_TAB, INTAKE_TABS, parseIntakeTab } from "./lib/tabs.js";
export type { IntakeTab } from "./lib/tabs.js";
export {
  INTAKE_TAB_LABELS,
  INTAKE_TAB_STATUSES,
  shouldShowOpenBadge,
  siblingTab,
  statusParamForTab,
  statusSetForTab,
  tabIndex,
} from "./lib/tabsModel.js";
export { INTAKE_AREA_LABEL, intakeDocumentTitle } from "./lib/title.js";
