// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Edit rights on the States tab (parity/specs/project-settings-states.md,
// PSET-028, PSET-034). Admins may always edit; members follow the
// project's members-may-edit-states flag; guests and outsiders never do.
// The flag defaults to on until the project row arrives.

import { ROLE_ADMIN, ROLE_MEMBER } from "../../core/session/permissions.js";
import type { ProjectRole } from "../project-settings/index.js";

/**
 * Whether this tab offers editing for the viewer. `flag` is the project's
 * members-may-edit-states setting; its default keeps the tab editable
 * while the project row is still loading, matching the tab's own gate.
 */
export function canEditStates(role: ProjectRole, flag: boolean): boolean {
  if (role === null) return false;
  if (role >= ROLE_ADMIN) return true;
  if (role < ROLE_MEMBER) return false;
  return flag;
}

/** Admins alone see the member-editing toggle (PSET-034). */
export function canToggleMemberEditing(role: ProjectRole): boolean {
  return role !== null && role >= ROLE_ADMIN;
}
