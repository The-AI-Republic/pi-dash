// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public API of the runners feature (NEWFRONT-540). Other features and
// routes import only from here. The area shell keeps this surface stable:
// later slices mount their screens through `RunnersAreaShell`, build their
// child links with `runnersPath`, and read data through the api hooks.

// In-app command-line setup (DESK-021): detect, install, self-approved login.
export {
  CLI_SETUP_BUSY,
  CLI_SETUP_UNAVAILABLE,
  createSetupLock,
  setUpCli,
  type CliSetupApi,
  type CliSetupDeps,
  type CliSetupInput,
  type CliSetupResult,
  type CliSetupStage,
} from "./cliSetup.js";
export { CliSetupPanel, CLI_SETUP_HEADING, CLI_SETUP_INTRO } from "./components/CliSetupPanel.js";

// Desktop runtime lifecycle (NEWFRONT-552). The root lifecycle (mounted in
// the app shell) owns sign-out registration; the hooks below are its API.
export { APPROVAL_MODE_STORAGE_PREFIX, restoreApprovalModes, stashApprovalModes } from "./runtime/approvalModes.js";
export type { ApprovalModeCarryInput } from "./runtime/approvalModes.js";
export { isManagedRunnerAvailable, projectAvailabilityQueryOptions, runtimeKeys } from "./runtime/queries.js";
export {
  CREDENTIAL_RENEW_BEFORE_MS,
  connectProject,
  disposeRuntimeForSignOut,
  EngineUnavailableError,
  ensureChatRuntime,
  getRuntimeAccount,
  HEARTBEAT_POLL_ATTEMPTS,
  HEARTBEAT_POLL_INTERVAL_MS,
  MANAGED_RUNNER_KIND,
  prepareAgentRun,
  refreshRuntime,
  resumeRuntime,
} from "./runtime/service.js";
export type { AgentRunTarget, HeartbeatPoll, RuntimeDeps } from "./runtime/service.js";
export { confirmRuntimeSignOut, registerSignOutPrompt } from "./runtime/signOutPrompt.js";
export { RuntimeSignOutDialog } from "./runtime/SignOutDialog.js";
export { registerRuntimeSignOutLifecycle, RuntimeSignOutLifecycle } from "./runtime/SignOutLifecycle.js";
export {
  AVAILABILITY_POLL_INTERVAL_MS,
  matchProjectIdentifier,
  RUNTIME_CONNECTED_TEXT,
  RUNTIME_PREPARING_TEXT,
  RUNTIME_REFRESH_INTERVAL_MS,
  RUNTIME_WAITING_TEXT,
  RuntimeSupervisor,
  selectRuntimeNotice,
} from "./runtime/Supervisor.js";
export type { RuntimeNoticeInput, RuntimeSupervisorProps } from "./runtime/Supervisor.js";

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
