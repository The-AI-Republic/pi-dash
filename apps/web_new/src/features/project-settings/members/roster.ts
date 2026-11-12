// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project members roster model (parity/specs/project-settings-members.md
// §"Member table", §"Search, filter and sort"; PSET-018–021). Pure data
// rules only: the page joins two reads, then asks this module which rows
// survive the search and role filter, in which order, and how each cell
// reads. Nothing here touches React, the router or the network.
//
// The table shows one row per project membership whose user is not a bot.
// The membership read carries only the user id, so identity facts (names,
// avatar, email) come from the workspace directory when it knows that id;
// an unknown id still renders a row so the roster never silently drops a
// member.

import type { ProjectMemberList, WorkspaceMember, WorkspaceMemberList } from "@pidash/api-client";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";

/** The three project roles, in the filter and sort order the spec names. */
export type MemberRoleKey = "admin" | "member" | "guest";

export interface MemberRoleOption {
  key: MemberRoleKey;
  /** Role number the backend stores for this level. */
  role: number;
  label: string;
}

/** One checkbox per project role, admin first (PSET-020). */
export const MEMBER_ROLE_OPTIONS: readonly MemberRoleOption[] = [
  { key: "admin", role: ROLE_ADMIN, label: "Admin" },
  { key: "member", role: ROLE_MEMBER, label: "Member" },
  { key: "guest", role: ROLE_GUEST, label: "Guest" },
];

/** Table columns in display order; every header carries a sort menu. */
export type MemberColumnKey = "full_name" | "display_name" | "email" | "role" | "joining_date";

export interface MemberColumnSpec {
  key: MemberColumnKey;
  label: string;
}

export const MEMBER_COLUMNS: readonly MemberColumnSpec[] = [
  { key: "full_name", label: "Full name" },
  { key: "display_name", label: "Display name" },
  { key: "email", label: "Email" },
  { key: "role", label: "Account type" },
  { key: "joining_date", label: "Joining date" },
];

export type MemberSortDirection = "asc" | "desc";

/** One applied column sort; null means the default server order. */
export interface MemberSort {
  column: MemberColumnKey;
  direction: MemberSortDirection;
}

export interface MemberRosterFilters {
  search: string;
  roles: readonly MemberRoleKey[];
  sort: MemberSort | null;
}

export const EMPTY_MEMBER_ROSTER_FILTERS: MemberRosterFilters = { search: "", roles: [], sort: null };

/** One rendered member row, already joined for the table. */
export interface MemberRosterRow {
  /** Membership id (what the writes slice targets). */
  membershipId: string;
  /** User id the profile link and identity join use. */
  userId: string;
  firstName: string;
  lastName: string;
  /** Display name exactly as the directory stored it; "" when none. */
  rawDisplayName: string;
  /** Full name text as the row shows it; never blank. */
  fullName: string;
  /** Display name, or the full name when the directory gave none. */
  displayName: string;
  /** Account email; empty when the directory lens omitted it. */
  email: string;
  /** Stored avatar path; resolve through the app's file URL helper. */
  avatarUrl: string | null;
  /** Role number the membership carries. */
  role: number;
  /** Role word: Admin, Member or Guest. */
  roleLabel: string;
  /** Membership creation timestamp, ISO-8601. */
  joinedAt: string;
}

const JOINING_FORMAT = new Intl.DateTimeFormat("en-US", { year: "numeric", month: "short", day: "numeric" });

/** Joining date for display; empty when the timestamp is unparseable. */
export function formatJoiningDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return JOINING_FORMAT.format(date);
}

/** Role word for a stored role number (PSET-018's account-type column). */
export function memberRoleLabel(role: number): string {
  if (role >= ROLE_ADMIN) return "Admin";
  if (role >= ROLE_MEMBER) return "Member";
  return "Guest";
}

/** The filter key a stored role number belongs to. */
export function memberRoleKey(role: number): MemberRoleKey {
  if (role >= ROLE_ADMIN) return "admin";
  if (role >= ROLE_MEMBER) return "member";
  return "guest";
}

function directoryEmail(entry: WorkspaceMember): string {
  return entry.member.email ?? "";
}

/**
 * Join project memberships to the workspace directory, drop bots, and
 * shape the table rows. Server order is kept; sorting is a separate step
 * so an empty sort leaves the roster exactly as delivered.
 */
export function buildMemberRoster(memberships: ProjectMemberList, directory: WorkspaceMemberList): MemberRosterRow[] {
  const byUserId = new Map<string, WorkspaceMember>();
  for (const entry of directory) byUserId.set(entry.member.id, entry);

  const rows: MemberRosterRow[] = [];
  for (const membership of memberships) {
    const entry = byUserId.get(membership.member);
    if (entry?.member.is_bot === true) continue;
    const joinedName = `${entry?.member.first_name ?? ""} ${entry?.member.last_name ?? ""}`.trim();
    const rawDisplayName = entry?.member.display_name ?? "";
    const displayName = rawDisplayName === "" ? joinedName : rawDisplayName;
    rows.push({
      membershipId: membership.id,
      userId: membership.member,
      firstName: entry?.member.first_name ?? "",
      lastName: entry?.member.last_name ?? "",
      fullName: joinedName === "" ? displayName : joinedName,
      rawDisplayName,
      displayName,
      email: entry === undefined ? "" : directoryEmail(entry),
      avatarUrl: entry?.member.avatar_url ?? null,
      role: membership.role,
      roleLabel: memberRoleLabel(membership.role),
      joinedAt: membership.created_at,
    });
  }
  return rows;
}

/**
 * Search (PSET-019): a case-insensitive substring over the display name
 * and the first-plus-last full name. A blank query admits everything.
 */
export function memberMatchesSearch(row: MemberRosterRow, search: string): boolean {
  const needle = search.trim().toLowerCase();
  if (needle === "") return true;
  const display = row.displayName.toLowerCase();
  const full = `${row.firstName} ${row.lastName}`.toLowerCase().trim();
  return display.includes(needle) || full.includes(needle);
}

/** Role filter (PSET-020): any checked role admits its rows; none admits all. */
export function memberMatchesRoleFilter(row: MemberRosterRow, roles: readonly MemberRoleKey[]): boolean {
  if (roles.length === 0) return true;
  return roles.includes(memberRoleKey(row.role));
}

function compareText(a: string, b: string): number {
  return a.toLowerCase().localeCompare(b.toLowerCase());
}

function compareRows(column: MemberColumnKey, a: MemberRosterRow, b: MemberRosterRow): number {
  switch (column) {
    case "full_name":
      return compareText(`${a.firstName} ${a.lastName}`.trim(), `${b.firstName} ${b.lastName}`.trim());
    case "display_name":
      return compareText(a.rawDisplayName, b.rawDisplayName);
    case "email":
      return compareText(a.email, b.email);
    case "role":
      return compareText(a.roleLabel, b.roleLabel);
    case "joining_date": {
      // A missing or unparseable date sorts as the oldest value, so it
      // trails ascending orders and leads descending ones.
      const aTime = Date.parse(a.joinedAt);
      const bTime = Date.parse(b.joinedAt);
      return (Number.isNaN(aTime) ? 0 : aTime) - (Number.isNaN(bTime) ? 0 : bTime);
    }
  }
}

/**
 * The visible rows: search and role filter first, then the chosen column
 * order. Ties keep their incoming order, and a cleared sort restores the
 * server order.
 */
export function applyMemberRosterFilters(
  rows: readonly MemberRosterRow[],
  filters: MemberRosterFilters
): MemberRosterRow[] {
  const matched = rows.filter(
    (row) => memberMatchesSearch(row, filters.search) && memberMatchesRoleFilter(row, filters.roles)
  );
  if (filters.sort === null) return matched;
  const { column, direction } = filters.sort;
  return [...matched].sort((a, b) => {
    const result = compareRows(column, a, b);
    return direction === "desc" ? -result : result;
  });
}

/** Toggle one role checkbox; an empty list means no role filter. */
export function toggleMemberRole(roles: readonly MemberRoleKey[], role: MemberRoleKey): MemberRoleKey[] {
  return roles.includes(role) ? roles.filter((entry) => entry !== role) : [...roles, role];
}
