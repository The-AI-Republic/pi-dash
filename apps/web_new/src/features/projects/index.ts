// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the projects feature. Other features import only from here.

export { projectKeys } from "./api/keys.js";
export {
  ARCHIVE_FAILED_TITLE,
  ARCHIVED_TITLE,
  CREATED_TITLE,
  DELETE_FAILED_TITLE,
  DELETED_TITLE,
  FAVORITE_ADDED_TITLE,
  FAVORITE_FAILED_TITLE,
  FAVORITE_REMOVED_TITLE,
  RESTORE_FAILED_TITLE,
  RESTORED_TITLE,
  selectPendingProjectId,
  useArchiveProject,
  useCreateProject,
  useDeleteProject,
  useRestoreProject,
  useToggleFavoriteProject,
} from "./api/mutations.js";
export type { CreateProjectInput, ProjectLifecycleInput, ToggleFavoriteInput } from "./api/mutations.js";
export { projectDetailsQueryOptions, projectsQueryOptions } from "./api/queries.js";
export { applyProjectOrder, PROJECT_ORDER_FAILED_TITLE, useUpdateProjectOrder } from "./api/projectOrder.js";
export type { ProjectOrderInput } from "./api/projectOrder.js";
export { JoinProjectDialog } from "./components/JoinProjectDialog.js";
export type { JoinProjectDialogProps } from "./components/JoinProjectDialog.js";
export { CreateProjectDialog } from "./components/CreateProjectDialog.js";
export type { CreateProjectDialogProps } from "./components/CreateProjectDialog.js";
export { DeleteProjectDialog, DELETE_CONFIRM_PHRASE } from "./components/DeleteProjectDialog.js";
export type { DeleteProjectDialogProps } from "./components/DeleteProjectDialog.js";
export { ProjectArchiveDialog, selectArchiveBody } from "./components/ProjectArchiveDialog.js";
export type { ArchiveDialogMode, ProjectArchiveDialogProps } from "./components/ProjectArchiveDialog.js";
export { ProjectCard, PROJECT_NETWORK_PRIVATE, formatCreationDate } from "./components/ProjectCard.js";
export type { ProjectCardProps } from "./components/ProjectCard.js";
export { ProjectLogo, selectLogoGlyph } from "./components/ProjectLogo.js";
export type { ProjectLogoProps } from "./components/ProjectLogo.js";
export { ProjectMemberStack, MEMBER_STACK_CAP, selectStackSplit } from "./components/ProjectMemberStack.js";
export type { MemberStackEntry, ProjectMemberStackProps } from "./components/ProjectMemberStack.js";
export { projectIssuesPath, projectSettingsPath, selectCardMenuEntries } from "./components/cardMenu.js";
export type { CardMenuCallbacks, CardMenuStanding } from "./components/cardMenu.js";
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
export {
  applyNavigationPrefs,
  mergeNavigationPrefs,
  projectPropertiesQueryOptions,
  selectProjectListCap,
  useProjectListCap,
  useProjectProperties,
  useTabPreferences,
  useWorkspaceProperties,
  workspacePropertiesQueryOptions,
} from "./api/tabPreferences.js";
export type { TabPreferencesHandle } from "./api/tabPreferences.js";
export { useUpdateProjectLimit, useUpdateProjectNavMode } from "./api/displayPreferences.js";
export type { ProjectLimitHandle, ProjectNavMode, ProjectNavModeHandle } from "./api/displayPreferences.js";
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
export {
  compareSidebarOrder,
  isAggregateRowActive,
  isProjectsGroupRoute,
  isSidebarProject,
  isSubnavEntryActive,
  moveProjectToIndex,
  projectOrderPatch,
  PROJECT_ORDER_STEP,
  resolveDropTarget,
  resolveProjectRowHref,
  resolveReorder,
  revealRow,
  selectSidebarProjects,
  selectSubnavEntries,
  sliceVisibleProjects,
  sortOrderForMove,
} from "./sidebar.js";
export type { ScrollRevealTarget, SidebarDropRow, SidebarDropTarget, SidebarSubnavEntry } from "./sidebar.js";
export { DefaultTabRedirect } from "./components/DefaultTabRedirect.js";
export type { DefaultTabRedirectProps } from "./components/DefaultTabRedirect.js";
export { LEAVE_CONFIRM_PHRASE } from "./components/LeaveProjectDialog.js";
export { LeaveProjectEntry } from "./components/LeaveProjectEntry.js";
export type { LeaveProjectDialogProps, LeaveProjectEntryProps } from "./components/LeaveProjectEntry.js";
export { ProjectQuickActions } from "./components/ProjectQuickActions.js";
export type { ProjectQuickActionsProps } from "./components/ProjectQuickActions.js";
export { ROW_LINK_COPIED_TITLE, SidebarProjectRow } from "./components/SidebarProjectRow.js";
export type { SidebarProjectRowProps } from "./components/SidebarProjectRow.js";
export { SidebarProjectsGroup } from "./components/SidebarProjectsGroup.js";
export type { SidebarProjectsGroupProps } from "./components/SidebarProjectsGroup.js";
export { ProjectSwitcher } from "./components/ProjectSwitcher.js";
export type { ProjectSwitcherProps, SwitcherProject } from "./components/ProjectSwitcher.js";
export { ProjectTabChrome } from "./components/ProjectTabChrome.js";
export type { ProjectTabChromeProps } from "./components/ProjectTabChrome.js";
export { ProjectTabStrip } from "./components/ProjectTabStrip.js";
export type { ProjectTabStripProps } from "./components/ProjectTabStrip.js";
export { PublishProjectEntry } from "./components/PublishProjectEntry.js";
export type { PublishProjectDialogProps, PublishProjectEntryProps } from "./components/PublishProjectEntry.js";
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
export {
  buildCreateLogoProps,
  buildCreatePayload,
  CREATE_DEFAULT_ICON,
  CREATE_ICON_CHOICES,
  CREATE_IDENTIFIER_MAX_LENGTH,
  CREATE_NAME_MAX_LENGTH,
  CREATE_NETWORK_PRIVATE,
  CREATE_NETWORK_PUBLIC,
  deriveProjectIdentifier,
  IDENTIFIER_TAKEN_CODE,
  IDENTIFIER_TAKEN_MESSAGE,
  NAME_TAKEN_CODE,
  NAME_TAKEN_MESSAGE,
  pickCreateIcon,
  resolveBrowserTimezone,
  sanitizeProjectIdentifier,
  selectCreateFieldErrors,
  validateCreateFields,
} from "./lib/project-create.js";
export type { CreateFieldErrors, CreateFieldValues, CreatePayloadValues } from "./lib/project-create.js";
