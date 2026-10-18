// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export {
  asString,
  CREATE_WORKSPACE_PATH,
  decidePageGate,
  isOnboardedProfile,
  loadGateInputs,
  loadLandingTarget,
  ONBOARDING_PATH,
  parseReturnPath,
  resolveLanding,
  safeNextPath,
} from "./guards.js";
export type { AuthPageKind, PageGateDecision, ParsedReturnPath } from "./guards.js";
export {
  useInstanceConfig,
  useMe,
  useMeSettings,
  usePermissions,
  useProfile,
  useSessionStatus,
  useWorkspace,
  useWorkspaces,
} from "./hooks.js";
export { createSessionMiddleware, sessionExpiredMiddleware } from "./middleware.js";
export { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, selectPermissions } from "./permissions.js";
export type { Permissions } from "./permissions.js";
export {
  decideWorkspaceRouteGate,
  getSessionClient,
  instanceConfigQueryOptions,
  instanceKeys,
  meQueryOptions,
  profileQueryOptions,
  selectWorkspace,
  selectWorkspaceRole,
  sessionKeys,
  setSessionClient,
  settingsQueryOptions,
  workspacesQueryOptions,
} from "./queries.js";
export type { WorkspaceGateDecision } from "./queries.js";
export { getRuntimeSignOutHooks, registerRuntimeSignOutHooks } from "./signOutHooks.js";
export type { RuntimeSignOutChoice, RuntimeSignOutHooks } from "./signOutHooks.js";
export { registerStoreReset, resetRegisteredStores, signOut, useSessionStore } from "./store.js";
export type { SessionStatus, StoreReset } from "./store.js";
