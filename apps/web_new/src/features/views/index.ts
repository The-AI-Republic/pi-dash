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
