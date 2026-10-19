// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members roster surface (NEWFRONT-537, rows WSET-008/011/014/042). This
// barrel belongs to the members chunk only: the settings shell never
// imports it, so the roster, its reads and its table stay out of the
// initial bundle. NEWFRONT-538 extends the roster through these seams.

export { MembersPage, MEMBERS_LOAD_FAILED_TEXT, MEMBERS_NO_MATCH_TEXT, MEMBERS_RETRY_LABEL } from "./MembersPage.js";
export type { MembersPageProps } from "./MembersPage.js";
export { MembersFilters } from "./MembersFilters.js";
export type { MembersFiltersProps } from "./MembersFilters.js";
export { MemberRow, formatJoiningDate } from "./MemberRow.js";
export type { MemberRowProps } from "./MemberRow.js";
export { MembersSkeleton, ROSTER_SKELETON_LINES } from "./MembersSkeleton.js";
export { registerMembersSettingsPage } from "./register.js";
export { MembersTable } from "./MembersTable.js";
export type { MembersTableProps } from "./MembersTable.js";
export {
  applyRosterFilters,
  authMediumLabel,
  columnOrderLabels,
  EMPTY_ROSTER_FILTERS,
  isSuspended,
  memberMatchesSearch,
  roleLabel,
  rosterCount,
  rosterFullName,
  rosterIdentityName,
  ROSTER_COLUMNS,
  ROSTER_ROLE_FILTERS,
  ROSTER_SORTABLE_COLUMNS,
  selectRosterMembers,
  toggleRoleFilter,
} from "./roster.js";
export type {
  RosterColumn,
  RosterColumnKey,
  RosterColumnSpec,
  RosterFilters,
  RosterRoleFilter,
  RosterTableColumn,
} from "./roster.js";
export {
  invitationsQueryOptions,
  rosterKeys,
  rosterQueryOptions,
  usePendingInvitations,
  useWorkspaceRoster,
} from "./api.js";
