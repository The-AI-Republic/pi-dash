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
