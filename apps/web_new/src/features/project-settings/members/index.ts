// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public surface of the project-settings members (read) area.

export { MembersSettings, ADD_MEMBER_LABEL, MEMBERS_NO_MATCH_TEXT } from "./MembersSettings.js";
export type { MembersSettingsProps } from "./MembersSettings.js";
export {
  applyMemberRosterFilters,
  buildMemberRoster,
  EMPTY_MEMBER_ROSTER_FILTERS,
  formatJoiningDate,
  MEMBER_COLUMNS,
  MEMBER_ROLE_OPTIONS,
  memberMatchesRoleFilter,
  memberMatchesSearch,
  memberRoleKey,
  memberRoleLabel,
  toggleMemberRole,
  type MemberColumnKey,
  type MemberColumnSpec,
  type MemberRoleKey,
  type MemberRoleOption,
  type MemberRosterFilters,
  type MemberRosterRow,
  type MemberSort,
  type MemberSortDirection,
} from "./roster.js";
export { clearMemberFilters, readMemberFilters, useMemberFilters, writeMemberFilters } from "./filters.js";
export {
  INVITE_MEMBERS_FAILED_TITLE,
  LEAVE_PROJECT_FAILED_TITLE,
  MEMBERS_LOAD_FAILED_FALLBACK,
  MEMBERS_LOAD_FAILED_TITLE,
  REMOVE_MEMBER_FAILED_TITLE,
  SAVE_DEFAULTS_FAILED_TITLE,
  UPDATE_ROLE_FAILED_TITLE,
  WRITE_FAILED_FALLBACK,
  memberFailureDetail,
  memberKeys,
  notifyMembersLoadFailure,
  projectMembersQueryOptions,
  useAddProjectMembers,
  useLeaveProject,
  useMemberEditor,
  useProjectDefaults,
  useProjectMemberships,
  useRemoveProjectMember,
  useSaveProjectDefaults,
  useUpdateProjectMemberRole,
  useWorkspaceDirectory,
  workspaceDirectoryQueryOptions,
} from "./api.js";
export type { AddMembersInput, MemberEditorState, MemberRoleInput, ProjectDefaultsPatch } from "./api.js";
export { InviteMembersDialog, INVITE_DIALOG_TITLE } from "./InviteMembersDialog.js";
export { MemberDefaults, ASSIGNEE_FIELD_LABEL, GUEST_VISIBILITY_LABEL, LEAD_FIELD_LABEL } from "./MemberDefaults.js";
export { MemberRowMenu, confirmationCopy, LEAVE_CONFIRM_VERB, REMOVE_CONFIRM_VERB } from "./MemberRowMenu.js";
export type { MemberRowControls } from "./MemberTable.js";
export {
  appendInviteDraftRow,
  canEditMemberRole,
  createInviteDraftRow,
  DEFAULTS_NONE_LABEL,
  defaultMemberOptions,
  defaultSlotLabel,
  INVITE_MEMBER_REQUIRED,
  inviteCandidates,
  inviteDraftPayload,
  memberRowAction,
  pickInviteDraftMember,
  removeInviteDraftRow,
  roleForWorkspaceRole,
  roleNumber,
  rolesForTarget,
  rolesForWorkspaceRole,
  roleWord,
  setInviteDraftRole,
  validateInviteDraftRows,
} from "./writes.js";
export type {
  DefaultMemberOption,
  InviteCandidate,
  InviteDraftRow,
  MemberEditor,
  MemberRowAction,
  MemberTarget,
} from "./writes.js";
