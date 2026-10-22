// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the intake feature (parity/specs/intake.md). Routes import
// from here only; sibling slices (list rows, create, detail, actions) add
// their own exports as they land.

export { INTAKE_DEFAULT_ORDER, intakeKeys } from "./api/keys.js";
export type { IntakeListFilters, IntakeListKeyInput } from "./api/keys.js";
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
export { IntakeEmptyState } from "./components/IntakeEmptyState.js";
export type { IntakeEmptyStateProps } from "./components/IntakeEmptyState.js";
export { IntakeGate } from "./components/IntakeGate.js";
export type { IntakeGateProps } from "./components/IntakeGate.js";
export { IntakeScreen } from "./components/IntakeScreen.js";
export type { IntakeScreenProps } from "./components/IntakeScreen.js";
export { INTAKE_LIST_PANEL_ID, IntakeTabs, openTabAccessibleName } from "./components/IntakeTabs.js";
export type { IntakeTabsProps } from "./components/IntakeTabs.js";
export { selectIntakeEmptyState } from "./lib/emptyStates.js";
export type { IntakeEmptyInput, IntakeEmptyState as IntakeEmptyKind } from "./lib/emptyStates.js";
export { intakeFeatureSettingsHref, intakeHref, retiredInboxHref } from "./lib/paths.js";
export {
  DEFAULT_INTAKE_SEARCH,
  intakeSearchFor,
  parseIntakeSearch,
  settleIntakeSearch,
  withoutSelection,
} from "./lib/search.js";
export type { IntakeSearch } from "./lib/search.js";
export {
  DEFAULT_INTAKE_TAB,
  INTAKE_TAB_LABELS,
  INTAKE_TABS,
  INTAKE_TAB_STATUSES,
  parseIntakeTab,
  shouldShowOpenBadge,
  siblingTab,
  statusParamForTab,
  statusSetForTab,
  tabIndex,
} from "./lib/tabs.js";
export type { IntakeTab } from "./lib/tabs.js";
export { INTAKE_AREA_LABEL, intakeDocumentTitle } from "./lib/title.js";
