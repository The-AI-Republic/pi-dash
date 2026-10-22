// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views feature barrel (parity/specs/views.md). Cross-feature imports
// enter here; routes render the list screen.

export { viewKeys } from "./api/keys.js";
export { projectViewsQueryOptions, workspaceViewsQueryOptions } from "./api/queries.js";
export { ProjectViewsListScreen } from "./components/ProjectViewsListScreen.js";
export { WorkspaceViewsListScreen } from "./components/WorkspaceViewsListScreen.js";
export type { WorkspaceViewsListScreenProps } from "./components/WorkspaceViewsListScreen.js";
export type { ProjectViewsListScreenProps } from "./components/ProjectViewsListScreen.js";
export { CreateViewDialog } from "./dialog/CreateViewDialog.js";
export type { CreateViewDialogProps } from "./dialog/CreateViewDialog.js";
export { ViewAccessField, OWNER_ACCESS, SHARED_ACCESS } from "./dialog/ViewAccessField.js";
export type { ViewAccessFieldProps } from "./dialog/ViewAccessField.js";
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
  projectViewsTitle,
  resolveProjectViewsBody,
  sortProjectViewsDefault,
  viewAccessLabel,
} from "./lib/projectViews.js";
export type { ProjectViewsBodyState, ProjectViewsBodyInput, ViewOwner } from "./lib/projectViews.js";
export {
  capWorkspaceViewName,
  matchesWorkspaceViewQuery,
  resolveWorkspaceCustomsState,
  selectWorkspaceViewGroups,
  sortWorkspaceViewsNewestFirst,
  WORKSPACE_DEFAULT_VIEWS,
  WORKSPACE_VIEW_NAME_LIMIT,
  workspaceCustomViewHref,
  workspaceDefaultViewHref,
  workspaceViewsTitle,
} from "./lib/workspaceViews.js";
export type { WorkspaceCustomsState, WorkspaceDefaultView, WorkspaceViewGroups } from "./lib/workspaceViews.js";
