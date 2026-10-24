// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export { createSpaceRuntime, spaceRuntime } from "./client.js";
export type { SpaceRuntime } from "./client.js";
export { spaceKeys } from "./keys.js";
export {
  readSpaceViewer,
  resolveLegacySpaceAnchor,
  spaceInstanceQueryOptions,
  spaceIssuesQueryOptions,
  spaceLabelsQueryOptions,
  spaceSettingsQueryOptions,
  spaceStatesQueryOptions,
  spaceViewerQueryOptions,
} from "./queries.js";
