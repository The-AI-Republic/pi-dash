// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

export { useMe, usePermissions, useSessionStatus, useWorkspace, useWorkspaces } from "./hooks.js";
export { createSessionMiddleware, sessionExpiredMiddleware } from "./middleware.js";
export { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER, selectPermissions } from "./permissions.js";
export type { Permissions } from "./permissions.js";
export {
  getSessionClient,
  meQueryOptions,
  selectWorkspace,
  selectWorkspaceRole,
  sessionKeys,
  setSessionClient,
  workspacesQueryOptions,
} from "./queries.js";
export { registerStoreReset, resetRegisteredStores, signOut, useSessionStore } from "./store.js";
export type { SessionStatus, StoreReset } from "./store.js";
