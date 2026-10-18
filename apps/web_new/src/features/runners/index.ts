// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the runners feature (NEWFRONT-540). Other features and
// routes import only from here. The area shell keeps this surface stable:
// later slices mount their screens through `RunnersAreaShell`, build their
// child links with `runnersPath`, and read data through the api hooks.
export { decideProjectRunnersAccess, decideWorkspaceRunnersAccess, type RunnersAccess } from "./access.js";
export { buildRunnersAreaValue, RunnersAreaProvider, useRunnersArea, type RunnersAreaValue } from "./area.js";
export { runnersKeys, type RunnersScope } from "./api/keys.js";
export { useDeleteRunner, useRevokeRunner } from "./api/mutations.js";
export {
  PODS_POLL_INTERVAL_MS,
  RUNNERS_POLL_INTERVAL_MS,
  runnersPodsQueryOptions,
  runnersQueryOptions,
  useRunners,
  useRunnersPods,
  useRunnersRail,
} from "./api/queries.js";
export {
  canRevokeRunner,
  DELETE_RUNNER_FAILED,
  REVOKE_RUNNER_FAILED,
  RUNNERS_EMPTY_TEXT,
  RUNNER_ACTION_FAILED_TITLE,
  runnerHeartbeat,
  runnerPlatform,
  runnerVersion,
  RunnersOverview,
} from "./components/RunnersOverview.js";
export type { RunnersOverviewProps } from "./components/RunnersOverview.js";
export { isProjectScope, RunnersAreaShell } from "./components/RunnersAreaShell.js";
export type { RunnersAreaShellProps } from "./components/RunnersAreaShell.js";
export {
  DELETE_RUNNER_BODY,
  DELETE_RUNNER_TITLE,
  REVOKE_RUNNER_BODY,
  REVOKE_RUNNER_TITLE,
  runnerConfirmCopy,
} from "./components/RunnerConfirmDialog.js";
export { RAIL_HEADER, RunnersRail } from "./components/RunnersRail.js";
export { RunnerStatusBadge, RunnerStatusDot, runnerStatusOf } from "./components/RunnerStatusBadge.js";
export { RUNNERS_DENIED_HEADING, RunnersNotAuthorized } from "./components/NotAuthorizedView.js";
export { runnersActiveTabKey, RUNNERS_TABS, RunnersTabStrip } from "./components/RunnersTabStrip.js";
export { RunnersProjectShell } from "./RunnersProjectShell.js";
export type { RunnersProjectShellProps } from "./RunnersProjectShell.js";
export { RunnersWorkspaceShell } from "./RunnersWorkspaceShell.js";
export type { RunnersWorkspaceShellProps } from "./RunnersWorkspaceShell.js";
export { runnersBase, runnersPath } from "./scope.js";
export type { RunnersScopeInput } from "./scope.js";
export { RUNNER_STATUS_LABELS, RUNNER_STATUS_TONES, runnerDisplayStatus, type RunnerStatus } from "./status.js";
