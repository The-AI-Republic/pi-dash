// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views feature barrel (parity/specs/views.md). Cross-feature imports
// enter here; routes render the list screen.

export { viewKeys } from "./api/keys.js";
export { projectViewsQueryOptions, workspaceViewsQueryOptions } from "./api/queries.js";
export { resetViewFavoriteIds, selectPendingViewId, useToggleViewFavorite } from "./api/viewFavorites.js";
export type { ToggleViewFavoriteInput } from "./api/viewFavorites.js";
export { ProjectViewsListScreen } from "./components/ProjectViewsListScreen.js";
export { loadSavedViewDetail, resolvedSavedViewDetail } from "./detail/lazy.js";
export { loadWorkspaceViewDetail, resolvedWorkspaceViewDetail } from "./detail/lazy.js";
export type { SavedViewScreenProps } from "./detail/SavedViewScreen.js";
export { SavedViewHeader } from "./detail/SavedViewHeader.js";
export type { SavedViewHeaderProps } from "./detail/SavedViewHeader.js";
export { SavedViewMissingState } from "./detail/SavedViewMissingState.js";
export type { SavedViewMissingStateProps } from "./detail/SavedViewMissingState.js";
export { SavedViewSwitcher, viewSwitcherOptions } from "./detail/SavedViewSwitcher.js";
export { ViewSwitcherMark } from "./detail/SavedViewSwitcher.js";
export type { SavedViewSwitcherProps } from "./detail/SavedViewSwitcher.js";
export type { SavedViewDetailParams } from "./detail/preload.js";
export type { WorkspaceViewDetailParams } from "./detail/workspacePreload.js";
export {
  MISSING_VIEW_BACK_LABEL,
  MISSING_VIEW_DESCRIPTION,
  MISSING_WORKSPACE_VIEW_BACK_LABEL,
  MISSING_VIEW_TITLE,
  saveAsTitle,
  savedViewDocumentTitle,
  savedViewLayout,
  savedViewTitle,
  showsPrivateLock,
} from "./detail/model.js";
export { projectViewDetailQueryOptions, workspaceViewDetailQueryOptions } from "./api/detail.js";
export { WorkspaceViewHeader } from "./detail/WorkspaceViewHeader.js";
export type { WorkspaceViewHeaderProps } from "./detail/WorkspaceViewHeader.js";
export { WorkspaceViewScreen } from "./detail/WorkspaceViewScreen.js";
export type { WorkspaceViewScreenProps } from "./detail/WorkspaceViewScreen.js";
export { WorkspaceViewSwitcher, workspaceSwitcherOptions } from "./detail/WorkspaceViewSwitcher.js";
export type { WorkspaceViewSwitcherProps } from "./detail/WorkspaceViewSwitcher.js";
export { WorkspaceViewsListScreen } from "./components/WorkspaceViewsListScreen.js";
export type { WorkspaceViewsListScreenProps } from "./components/WorkspaceViewsListScreen.js";
export { WorkspaceViewMenu, selectWorkspaceViewMenuEntries } from "./components/WorkspaceViewMenu.js";
export type {
  WorkspaceViewMenuCallbacks,
  WorkspaceViewMenuProps,
  WorkspaceViewMenuStanding,
} from "./components/WorkspaceViewMenu.js";
export { WorkspaceViewsTabStrip } from "./components/WorkspaceViewsTabStrip.js";
export type { WorkspaceViewsTabStripProps } from "./components/WorkspaceViewsTabStrip.js";
export type { ProjectViewsListScreenProps } from "./components/ProjectViewsListScreen.js";
export { CreateViewDialog, UpdateViewDialog } from "./dialog/CreateViewDialog.js";
export type { CreateViewDialogProps, SaveViewPrefill, UpdateViewDialogProps } from "./dialog/CreateViewDialog.js";
export { WorkspaceViewDialog } from "./dialog/WorkspaceViewDialog.js";
export type { WorkspaceViewDialogProps } from "./dialog/WorkspaceViewDialog.js";
export { DeleteWorkspaceViewDialog } from "./dialog/DeleteWorkspaceViewDialog.js";
export type { DeleteWorkspaceViewDialogProps } from "./dialog/DeleteWorkspaceViewDialog.js";
export { ViewAccessField, OWNER_ACCESS, SHARED_ACCESS } from "./dialog/ViewAccessField.js";
export type { ViewAccessFieldProps } from "./dialog/ViewAccessField.js";
export { ViewAccessSummary } from "./dialog/ViewAccessSummary.js";
export type { ViewAccessSummaryProps } from "./dialog/ViewAccessSummary.js";
export { DeleteViewDialog, DELETE_VIEW_BODY, DELETE_VIEW_TITLE } from "./dialog/DeleteViewDialog.js";
export type { DeleteViewDialogProps } from "./dialog/DeleteViewDialog.js";
export { PublishViewDialog } from "./dialog/PublishViewDialog.js";
export type { PublishViewDialogProps } from "./dialog/PublishViewDialog.js";
export { ViewIconPicker, ViewLogoMark } from "./dialog/ViewIconPicker.js";
export type { ViewIconPickerProps, ViewLogoProps } from "./dialog/ViewIconPicker.js";
export { validateViewTitle, VIEW_TITLE_MAX_LENGTH } from "./dialog/viewTitle.js";
export { ViewsPaletteEntries } from "./palette/ViewsPaletteEntries.js";
export type { ViewsPaletteEntriesProps } from "./palette/ViewsPaletteEntries.js";
export {
  buildViewCreateCommand,
  buildViewJumpCommand,
  VIEW_CREATE_COMMAND_ID,
  VIEW_CREATE_SEQUENCE,
  VIEW_CREATE_TITLE,
  VIEW_JUMP_COMMAND_ID,
  VIEW_JUMP_PAGE_ID,
  VIEW_JUMP_SEQUENCE,
  VIEW_JUMP_TITLE,
} from "./palette/viewCommands.js";
export type { ViewCreateCommandContext, ViewJumpCommandContext } from "./palette/viewCommands.js";
export { projectViewsJumpPage, VIEW_JUMP_EMPTY_TEXT, VIEW_JUMP_PAGE_PLACEHOLDER } from "./palette/viewJumpPage.js";
export type { ViewJumpPageArgs } from "./palette/viewJumpPage.js";
export {
  PROJECT_VIEWS_CRUMB,
  VIEW_ACCESS_SHARED,
  findViewOwner,
  isSharedView,
  isViewLive,
  projectViewHref,
  projectViewsFeatureHref,
  projectViewsListHref,
  projectViewsTitle,
  resolveProjectViewsBody,
  sortProjectViewsDefault,
  viewAccessLabel,
} from "./lib/projectViews.js";
export type { ProjectViewsBodyState, ProjectViewsBodyInput, ViewOwner } from "./lib/projectViews.js";
export {
  canDeleteProjectView,
  canEditProjectView,
  canStarProjectView,
  findViewFavoriteId,
  flipViewFavorite,
  isViewFavorited,
  removeProjectViewRow,
  replaceProjectViewRow,
  selectProjectViewMenuItems,
  setProjectViewAnchor,
  withViewAnchor,
} from "./lib/projectViewActions.js";
export type { ProjectViewMenuItemId } from "./lib/projectViewActions.js";
export {
  capWorkspaceViewName,
  matchesWorkspaceViewQuery,
  resolveWorkspaceCustomsState,
  selectWorkspaceViewGroups,
  sortWorkspaceViewsNewestFirst,
  WORKSPACE_DEFAULT_VIEWS,
  WORKSPACE_VIEW_NAME_LIMIT,
  WORKSPACE_VIEWS_CRUMB,
  workspaceCustomViewHref,
  workspaceDefaultViewHref,
  workspaceViewsListHref,
  workspaceViewsTitle,
} from "./lib/workspaceViews.js";
export type { WorkspaceCustomsState, WorkspaceDefaultView, WorkspaceViewGroups } from "./lib/workspaceViews.js";
