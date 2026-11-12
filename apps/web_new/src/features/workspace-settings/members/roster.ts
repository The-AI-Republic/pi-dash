// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pure roster rules (parity/specs/workspace-settings.md §7–§9,
// WSET-008/011). Everything here is data in, data out: the page calls one
// selector chain and the tests pin each rule on its own.
//
// Rules restated from the spec:
// - the count chip counts non-bot memberships, suspended rows included;
// - search is a case-insensitive substring over first name, last name,
//   display name and email, and narrows whatever the role filter left;
// - the role filter toggles Admin/Member/Guest/Suspended; Suspended means
//   `is_active` false, the three roles match active rows by number, and an
//   empty filter lets every non-bot row through;
// - rows stay in server order inside two groups, active first, suspended
//   last; a chosen column order applies inside the same rule.

import type { WorkspaceInvitation, WorkspaceRosterMember, WorkspaceRosterUser } from "@pidash/api-client";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/index.js";

export type RosterRoleFilter = "admin" | "member" | "guest" | "suspended";

/** Display order of the filter toggles. */
export const ROSTER_ROLE_FILTERS: readonly { key: RosterRoleFilter; label: string }[] = [
  { key: "admin", label: "Admin" },
  { key: "member", label: "Member" },
  { key: "guest", label: "Guest" },
  { key: "suspended", label: "Suspended" },
];

export type RosterColumn = "full_name" | "display_name" | "email" | "role" | "joining_date";

export interface RosterColumnSpec {
  key: RosterColumn;
  label: string;
}

/**
 * Table columns in display order. The identity cell carries the avatar and
 * full name, then display name, email, account type, authentication and the
 * joining date — the order the parity driver reads cells in.
 */
export type RosterColumnKey = RosterColumn | "authentication";

export interface RosterTableColumn {
  key: RosterColumnKey;
  label: string;
}

export const ROSTER_COLUMNS: readonly RosterTableColumn[] = [
  { key: "full_name", label: "Full name" },
  { key: "display_name", label: "Display name" },
  { key: "email", label: "Email address" },
  { key: "role", label: "Account type" },
  { key: "authentication", label: "Authentication" },
  { key: "joining_date", label: "Joining date" },
];

/**
 * Headers with an ascending/descending menu. Spec §7 names full name,
 * display name, email, role and joining date; authentication has no menu.
 */
export const ROSTER_SORTABLE_COLUMNS: readonly RosterColumnSpec[] = [
  { key: "full_name", label: "Full name" },
  { key: "display_name", label: "Display name" },
  { key: "email", label: "Email address" },
  { key: "role", label: "Account type" },
  { key: "joining_date", label: "Joining date" },
];

export interface RosterFilters {
  search: string;
  roles: readonly RosterRoleFilter[];
  order: { column: RosterColumn; direction: "asc" | "desc" } | null;
}

export const EMPTY_ROSTER_FILTERS: RosterFilters = { search: "", roles: [], order: null };

const AUTH_MEDIUM_LABELS: ReadonlyMap<string, string> = new Map([
  ["email", "Email"],
  ["magic-code", "Magic code"],
  ["github", "GitHub"],
  ["gitlab", "GitLab"],
  ["google", "Google"],
  ["gitea", "Gitea"],
]);

/**
 * Human label for a stored sign-in medium; null when the account never
 * signed in or the stored value is not one the column names (§8). The
 * roster hides the cell rather than echoing a raw provider code.
 */
export function authMediumLabel(medium: string | null | undefined): string | null {
  if (medium === null || medium === undefined || medium === "") return null;
  return AUTH_MEDIUM_LABELS.get(medium) ?? null;
}

/** Role label for an active member; numeric roles are the backend's. */
export function roleLabel(role: number): string {
  if (role >= ROLE_ADMIN) return "Admin";
  if (role >= ROLE_MEMBER) return "Member";
  if (role >= ROLE_GUEST) return "Guest";
  return "Guest";
}

export function isSuspended(row: WorkspaceRosterMember): boolean {
  return row.is_active === false;
}

/** Name used for naming the avatar and profile link. */
export function rosterIdentityName(user: WorkspaceRosterUser): string {
  if (user.display_name !== "") return user.display_name;
  if (user.email !== "") return user.email;
  return `${user.first_name} ${user.last_name}`.trim();
}

/**
 * One-letter avatar fallback (§8): the opening letter of the email, else
 * of the display name, else the first name. `?` when nothing is known.
 */
export function rosterAvatarInitial(user: WorkspaceRosterUser): string {
  for (const value of [user.email, user.display_name, user.first_name]) {
    const letter = value.trim().charAt(0).toUpperCase();
    if (letter !== "") return letter;
  }
  return "?";
}

/** Full name as the roster shows it; never blank. */
export function rosterFullName(user: WorkspaceRosterUser): string {
  const joined = `${user.first_name} ${user.last_name}`.trim();
  return joined === "" ? rosterIdentityName(user) : joined;
}

/** Every non-bot membership, in server order. */
export function selectRosterMembers(rows: readonly WorkspaceRosterMember[]): WorkspaceRosterMember[] {
  return rows.filter((row) => !row.member.is_bot);
}

/** The count chip's number: non-bot memberships, suspended included. */
export function rosterCount(rows: readonly WorkspaceRosterMember[]): number {
  return selectRosterMembers(rows).length;
}

/** Case-insensitive substring over first, last, display name and email. */
export function memberMatchesSearch(user: WorkspaceRosterUser, search: string): boolean {
  const needle = search.trim().toLowerCase();
  if (needle === "") return true;
  const haystack = [user.first_name, user.last_name, user.display_name, user.email].join(" ").toLowerCase();
  return haystack.includes(needle);
}

/**
 * Pending invitations are searched by email only and ignore the role
 * filter (§7). The roster shows none of them in this slice, but §9 makes
 * the no-match note wait until every invitation is excluded too.
 */
export function invitationMatchesSearch(invitation: WorkspaceInvitation, search: string): boolean {
  const needle = search.trim().toLowerCase();
  if (needle === "") return true;
  return invitation.email.toLowerCase().includes(needle);
}

/** Whether any invitation survives the search — no-match gates on it (§9). */
export function hasMatchingInvitation(invitations: readonly WorkspaceInvitation[], search: string): boolean {
  return invitations.some((invitation) => invitationMatchesSearch(invitation, search));
}

function matchesRole(row: WorkspaceRosterMember, roles: readonly RosterRoleFilter[]): boolean {
  if (roles.length === 0) return true;
  if (isSuspended(row)) return roles.includes("suspended");
  if (roles.includes("admin") && row.role >= ROLE_ADMIN) return true;
  if (roles.includes("member") && row.role >= ROLE_MEMBER && row.role < ROLE_ADMIN) return true;
  if (roles.includes("guest") && row.role < ROLE_MEMBER) return true;
  return false;
}

function compareRows(column: RosterColumn, a: WorkspaceRosterMember, b: WorkspaceRosterMember): number {
  switch (column) {
    case "full_name":
      return rosterFullName(a.member).toLowerCase().localeCompare(rosterFullName(b.member).toLowerCase());
    case "display_name":
      return a.member.display_name.toLowerCase().localeCompare(b.member.display_name.toLowerCase());
    case "email":
      return a.member.email.toLowerCase().localeCompare(b.member.email.toLowerCase());
    case "role":
      return a.role - b.role;
    case "joining_date": {
      const aTime = Date.parse(a.created_at);
      const bTime = Date.parse(b.created_at);
      return (Number.isNaN(aTime) ? 0 : aTime) - (Number.isNaN(bTime) ? 0 : bTime);
    }
  }
}

/**
 * Apply search and role filters, then order active rows above suspended
 * ones. A chosen column order sorts inside the two groups; without one the
 * server order stands. Ties keep their incoming order.
 */
export function applyRosterFilters(
  rows: readonly WorkspaceRosterMember[],
  filters: RosterFilters
): WorkspaceRosterMember[] {
  const matched = selectRosterMembers(rows).filter(
    (row) => memberMatchesSearch(row.member, filters.search) && matchesRole(row, filters.roles)
  );
  const ordered =
    filters.order === null
      ? matched
      : [...matched].sort((a, b) => {
          const result = compareRows(filters.order!.column, a, b);
          return filters.order!.direction === "desc" ? -result : result;
        });
  const active = ordered.filter((row) => !isSuspended(row));
  const suspended = ordered.filter((row) => isSuspended(row));
  return [...active, ...suspended];
}

/** Toggle one role filter; the empty list means "no filter". */
export function toggleRoleFilter(roles: readonly RosterRoleFilter[], role: RosterRoleFilter): RosterRoleFilter[] {
  return roles.includes(role) ? roles.filter((entry) => entry !== role) : [...roles, role];
}

/** Header menu labels for one column: the current direction reads checked. */
export function columnOrderLabels(spec: RosterColumnSpec): { asc: string; desc: string } {
  if (spec.key === "role") return { asc: "Guest → Admin", desc: "Admin → Guest" };
  if (spec.key === "joining_date") return { asc: "Oldest → Newest", desc: "Newest → Oldest" };
  return { asc: "A → Z", desc: "Z → A" };
}
