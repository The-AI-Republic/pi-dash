// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).

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
