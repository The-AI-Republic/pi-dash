// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Worklog eligibility (parity/specs/issues.md §16/§23, ISS-206, cloud
// leg). Logging is for admins and assignees on live, non-intake items —
// guests never see the control, whatever their assignment. Managing an
// entry (edit, delete) is for its author and for admins, matching the
// server's admin-or-creator rule; the server enforces both rules, these
// only gate the UI.

import type { WorklogEntry } from "@pidash/api-client";

import { ROLE_ADMIN, ROLE_MEMBER } from "../../../../../core/session/permissions.js";

export interface WorklogStanding {
  /** Project role (20 admin, 15 member, 5 guest); null when unknown. */
  memberRole: number | null;
  viewerId: string | null;
  assigneeIds: readonly string[];
  isIntake: boolean;
  archived: boolean;
}

/** Whether the viewer gets the log-time control on this work item. */
export function canLogWorklog(standing: WorklogStanding): boolean {
  if (standing.isIntake || standing.archived) return false;
  if (standing.memberRole !== ROLE_ADMIN && standing.memberRole !== ROLE_MEMBER) return false;
  if (standing.viewerId === null) return false;
  return standing.memberRole === ROLE_ADMIN || standing.assigneeIds.includes(standing.viewerId);
}

export interface WorklogEntryStanding {
  viewerId: string | null;
  /** Project role (20 admin, 15 member, 5 guest); null when unknown. */
  memberRole: number | null;
  entry: Pick<WorklogEntry, "actor">;
  archived: boolean;
}

/** Whether the viewer may edit or remove one entry: its author, or an admin, on a live item. */
export function canManageWorklogEntry(standing: WorklogEntryStanding): boolean {
  if (standing.archived) return false;
  if (standing.memberRole === ROLE_ADMIN) return true;
  if (standing.viewerId === null) return false;
  return standing.entry.actor === standing.viewerId;
}
