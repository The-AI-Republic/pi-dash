// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline edit standing (parity/specs/issues.md §6, ISS-050–055/058).
// Moving, resizing, reordering, planting and quick-adding need project
// admin or member standing; a workspace admin qualifies even without a
// project membership row, and an archived project grants nothing.

import { ROLE_ADMIN, ROLE_MEMBER } from "../../../core/session/permissions.js";

/** Standing inputs behind every drag and add affordance. */
export interface TimelineStanding {
  /** Workspace role number, or null outside the workspace. */
  workspaceRole: number | null;
  /** The caller's project role, or null when not a project member. */
  projectRole: number | null;
  /** True when the project is archived (all edits hidden). */
  archived: boolean;
}

/** True when the caller may drag, resize, reorder or add on the timeline. */
export function canEditTimeline(standing: TimelineStanding): boolean {
  if (standing.archived) return false;
  if (standing.workspaceRole !== null && standing.workspaceRole >= ROLE_ADMIN) return true;
  return standing.projectRole !== null && standing.projectRole >= ROLE_MEMBER;
}
