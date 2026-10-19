// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Timeline edit standing (parity/specs/issues.md §6, ISS-050–055/058).
// Moving, resizing, reordering, planting and quick-adding need project
// admin or member standing, and an archived project grants nothing. A
// workspace admin gets no standing on its own: the server's project
// permission check requires an active ProjectMember row, so a workspace
// admin without one cannot write dates or ranks. Their elevated role
// only applies through the membership that assigns it (ISS-058).

import { ROLE_MEMBER } from "../../../core/session/permissions.js";

/** Standing inputs behind every drag and add affordance. */
export interface TimelineStanding {
  /** The caller's project role, or null when not a project member. */
  projectRole: number | null;
  /** True when the project is archived (all edits hidden). */
  archived: boolean;
}

/** True when the caller may drag, resize, reorder or add on the timeline. */
export function canEditTimeline(standing: TimelineStanding): boolean {
  if (standing.archived) return false;
  return standing.projectRole !== null && standing.projectRole >= ROLE_MEMBER;
}
