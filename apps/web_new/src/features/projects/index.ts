// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the projects feature. Other features import only from here.

export { projectKeys } from "./api/keys.js";
export {
  projectDetailsQueryOptions,
  projectPropertiesQueryOptions,
  projectsQueryOptions,
  useProjectProperties,
  useWorkspaceProperties,
  workspacePropertiesQueryOptions,
} from "./api/queries.js";
export { ProjectCard } from "./components/ProjectCard.js";
export type { ProjectCardProps } from "./components/ProjectCard.js";
export { ProjectsEmptyState } from "./components/ProjectsEmptyState.js";
export type { ProjectsEmptyStateProps, ProjectsEmptyVariant } from "./components/ProjectsEmptyState.js";
export { ProjectsListHeader } from "./components/ProjectsListHeader.js";
export type { ProjectsListHeaderProps } from "./components/ProjectsListHeader.js";
export { ProjectsListScreen, selectListBody, selectViewRows } from "./components/ProjectsListScreen.js";
export type {
  CreateDialogControls,
  ProjectsListBody,
  ProjectsListScreenProps,
} from "./components/ProjectsListScreen.js";
export { ProjectsListView } from "./components/ProjectsListView.js";
export type { ProjectsListViewProps } from "./components/ProjectsListView.js";
export { applyNavigationPrefs, mergeNavigationPrefs, useTabPreferences } from "./api/tabPreferences.js";
export type { TabPreferencesHandle } from "./api/tabPreferences.js";
export {
  countFittingTabs,
  DEFAULT_TAB_KEY,
  EPIC_TAB_KEY,
  isProjectAdmin,
  isProjectAuthorized,
  isTabbedMode,
  parseNavigationPrefs,
  PROJECT_TAB_DEFS,
  resolveActiveTabKey,
  resolveOverflow,
  selectCopyPath,
  selectDefaultTarget,
  selectProjectTabs,
  selectQuickActions,
  selectSwitcherTarget,
  tabHref,
} from "./tabs.js";
export type {
  ProjectTab,
  ProjectTabDef,
  ProjectTabFlags,
  QuickActionEntry,
  QuickActionId,
  StripWorkItem,
  TabPreferences,
} from "./tabs.js";
export { DefaultTabRedirect } from "./components/DefaultTabRedirect.js";
export type { DefaultTabRedirectProps } from "./components/DefaultTabRedirect.js";
export { LeaveProjectDialog, LEAVE_CONFIRM_PHRASE } from "./components/LeaveProjectDialog.js";
export type { LeaveProjectDialogProps } from "./components/LeaveProjectDialog.js";
export { ProjectQuickActions } from "./components/ProjectQuickActions.js";
export type { ProjectQuickActionsProps } from "./components/ProjectQuickActions.js";
export { ProjectSwitcher } from "./components/ProjectSwitcher.js";
export type { ProjectSwitcherProps, SwitcherProject } from "./components/ProjectSwitcher.js";
export { ProjectTabChrome } from "./components/ProjectTabChrome.js";
export type { ProjectTabChromeProps } from "./components/ProjectTabChrome.js";
export { ProjectTabStrip } from "./components/ProjectTabStrip.js";
export type { ProjectTabStripProps } from "./components/ProjectTabStrip.js";
export { PublishProjectDialog } from "./components/PublishProjectDialog.js";
export type { PublishProjectDialogProps } from "./components/PublishProjectDialog.js";
export { TabAreaMount } from "./components/TabAreaMount.js";
export { TabBar } from "./components/TabBar.js";
export type { TabBarProps } from "./components/TabBar.js";
export { workspaceMembersQueryOptions } from "./api/members.js";
export { ProjectsAppliedFilters } from "./components/ProjectsAppliedFilters.js";
export type { ProjectsAppliedFiltersProps } from "./components/ProjectsAppliedFilters.js";
export { ProjectsCustomDateDialog } from "./components/ProjectsCustomDateDialog.js";
export type { CustomDateMode, ProjectsCustomDateDialogProps } from "./components/ProjectsCustomDateDialog.js";
export { ProjectsFilterPanel } from "./components/ProjectsFilterPanel.js";
export type { ProjectsFilterPanelProps } from "./components/ProjectsFilterPanel.js";
export { ProjectsListSearch } from "./components/ProjectsListSearch.js";
export type { ProjectsListSearchProps } from "./components/ProjectsListSearch.js";
export { ProjectsSortMenu } from "./components/ProjectsSortMenu.js";
export type { ProjectsSortMenuProps } from "./components/ProjectsSortMenu.js";
export {
  ACCESS_OPTIONS,
  countActiveListFilters,
  CREATED_PRESETS,
  DEFAULT_LIST_SORT,
  EMPTY_LIST_FILTERS,
  filterListRows,
  formatCreatedChip,
  freshListFilters,
  hasNarrowingListFilters,
  isCustomCreatedValue,
  LIST_SORT_OPTIONS,
  matchesCreatedFilter,
  matchesListFilters,
  matchesListSearch,
  selectMemberOptions,
  sortDirectionLocked,
  sortKeyLabel,
  sortListRows,
} from "./lib/list-display.js";
export type {
  ListFilters,
  ListSort,
  ListSortDirection,
  ListSortKey,
  MemberOption,
  PersonFilterDimension,
  ValueFilterDimension,
} from "./lib/list-display.js";
export { useListDisplayStore } from "./store/list-display.js";
export type { WorkspaceListDisplay } from "./store/list-display.js";
