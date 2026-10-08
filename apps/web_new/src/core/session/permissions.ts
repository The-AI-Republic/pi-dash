// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Role checks (Data layer page). All permission questions go through
// usePermissions, never ad hoc. Role numbers are the backend's workspace
// roles (pi_dash/core/permissions.py): Admin 20, Member 15, Guest 5.

export const ROLE_ADMIN = 20;
export const ROLE_MEMBER = 15;
export const ROLE_GUEST = 5;

export interface Permissions {
  /** The raw role, or null outside the workspace. */
  role: number | null;
  /** Any active membership in the workspace. */
  isMember: boolean;
  /** Member or admin: may create and edit. Guests are read-only. */
  canEdit: boolean;
  /** Workspace admin. */
  isAdmin: boolean;
}

const NO_PERMISSIONS: Permissions = {
  role: null,
  isMember: false,
  canEdit: false,
  isAdmin: false,
};

/** Derive the coarse client-side permission set from a workspace role. */
export function selectPermissions(role: number | null | undefined): Permissions {
  if (role == null) return { ...NO_PERMISSIONS };
  return {
    role,
    isMember: true,
    canEdit: role >= ROLE_MEMBER,
    isAdmin: role >= ROLE_ADMIN,
  };
}
