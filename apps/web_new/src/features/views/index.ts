// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Views feature barrel (parity/specs/views.md). Cross-feature imports
// enter here; routes render the list screen.

export { viewKeys } from "./api/keys.js";
export { projectViewsQueryOptions } from "./api/queries.js";
export { ProjectViewsListScreen } from "./components/ProjectViewsListScreen.js";
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
