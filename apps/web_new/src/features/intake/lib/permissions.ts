// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Intake edit permissions (parity/specs/intake.md INT-026). Pure: who may
// change the content (title, description, versions) and who may change
// the properties (assignees, priority, due date, labels). Admins and the
// creator edit; a guest creator is limited to the content; everyone else
// reads; resolved requests are read-only for every viewer.

import { INTAKE_STATUS } from "@pidash/api-client";

export interface IntakeStanding {
  /** Project admin standing. */
  isAdmin: boolean;
  /** The viewer created this request. */
  isCreator: boolean;
  /** Guest project standing. */
  isGuest: boolean;
  /** Intake status of the request. */
  status: number;
}

export interface IntakeEditability {
  /** Title, description and versions may change. */
  canEditContent: boolean;
  /** Assignees, priority, due date and labels may change. */
  canEditProperties: boolean;
}

/** Accepted, declined and duplicate requests are resolved (read-only). */
export function isResolvedIntakeStatus(status: number): boolean {
  return status === INTAKE_STATUS.accepted || status === INTAKE_STATUS.declined || status === INTAKE_STATUS.duplicate;
}

export function selectIntakeEditability(standing: IntakeStanding): IntakeEditability {
  if (isResolvedIntakeStatus(standing.status)) return { canEditContent: false, canEditProperties: false };
  const canEditContent = standing.isAdmin || standing.isCreator;
  const canEditProperties = standing.isAdmin || (standing.isCreator && !standing.isGuest);
  return { canEditContent, canEditProperties };
}

export interface IntakeTriageStanding {
  /** Project admin standing. */
  isAdmin: boolean;
  /** Admin or member standing: the overflow menu's viewers. */
  isAuthorized: boolean;
  /** The viewer created this request. */
  isCreator: boolean;
  /** Intake status of the request. */
  status: number;
}

export interface IntakeTriageVisibility {
  /** Accept/decline buttons and the overflow menu on an open request. */
  showTriage: boolean;
  /** Delete entry inside that menu. */
  showDelete: boolean;
}

/**
 * Triage visibility (parity/specs/intake.md INT-020–023, 025). Admins
 * and members see the buttons and the menu on open requests; a member
 * without admin rights is refused with a notice on click instead of
 * losing the controls. Delete shows for an admin or the creator, and
 * nothing triage-related shows once the request is resolved.
 */
export function selectIntakeTriageVisibility(standing: IntakeTriageStanding): IntakeTriageVisibility {
  if (isResolvedIntakeStatus(standing.status)) return { showTriage: false, showDelete: false };
  if (!standing.isAuthorized) return { showTriage: false, showDelete: false };
  return { showTriage: true, showDelete: standing.isAdmin || standing.isCreator };
}
