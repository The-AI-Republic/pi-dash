// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Role choices for the members page (parity/specs/workspace-settings.md
// §10–§12, WSET-009/010/012). An actor may hand out only the roles at or
// below their own workspace role, so an admin chooses from all three and a
// lower role never sees a control at all. Pure: the invite rows, the
// member rows and the pending rows all ask the same two questions.

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/index.js";

export interface AssignableRole {
  /** Numeric role the server stores. */
  value: number;
  /** Label the control renders. */
  label: string;
}

/** Canonical order: the least privileged first, an admin last. */
export const ASSIGNABLE_ROLES: readonly AssignableRole[] = [
  { value: ROLE_GUEST, label: "Guest" },
  { value: ROLE_MEMBER, label: "Member" },
  { value: ROLE_ADMIN, label: "Admin" },
];

/** Role a new invite row starts on (§10). */
export const DEFAULT_INVITE_ROLE = ROLE_MEMBER;

/**
 * Roles the actor may assign: every role whose number does not exceed the
 * actor's own. A missing role assigns nothing.
 */
export function assignableRoles(actorRole: number | null): readonly AssignableRole[] {
  if (actorRole === null) return [];
  return ASSIGNABLE_ROLES.filter((role) => role.value <= actorRole);
}

/** Labels in display order, for a control's option list. */
export function assignableRoleLabels(actorRole: number | null): string[] {
  return assignableRoles(actorRole).map((role) => role.label);
}

/** Stored role number for a label, or null when the actor may not assign it. */
export function roleValueForLabel(actorRole: number | null, label: string): number | null {
  return assignableRoles(actorRole).find((role) => role.label === label)?.value ?? null;
}

/** Whether an offer would exceed the actor's own role (§10–§12). */
export function isAboveActor(actorRole: number | null, candidate: number): boolean {
  if (actorRole === null) return true;
  return candidate > actorRole;
}

/** The role a new invite row starts on: Member when the actor may assign it, else the highest they may. */
export function initialInviteRole(actorRole: number | null): number {
  const options = assignableRoles(actorRole);
  if (options.length === 0) return DEFAULT_INVITE_ROLE;
  return options.some((role) => role.value === DEFAULT_INVITE_ROLE)
    ? DEFAULT_INVITE_ROLE
    : options[options.length - 1]!.value;
}

/** Label for any stored role number; an unknown number reads Guest like the roster. */
export function roleLabelFor(role: number): string {
  const match = ASSIGNABLE_ROLES.find((entry) => entry.value === role);
  if (match !== undefined) return match.label;
  return role > ROLE_ADMIN ? "Admin" : role > ROLE_MEMBER ? "Member" : "Guest";
}
