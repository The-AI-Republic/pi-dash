// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Write-side rules of the Members tab (parity/specs/project-settings-members.md
// §"Add members", §"Inline role changes", §"Remove and leave-self",
// §"Project defaults"; PSET-022–026). Pure data rules: which colleagues the
// invite picker may offer, which role each row may grant, who may edit a
// role cell, and which members a defaults picker lists. Nothing here touches
// React or the network.

import type { ProjectMemberList, WorkspaceMember, WorkspaceMemberList } from "@pidash/api-client";

import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../../core/session/permissions.js";
import { MEMBER_ROLE_OPTIONS, type MemberRoleOption, type MemberRosterRow } from "./roster.js";

/** One colleague waiting to be added, as the dialog holds them. */
export interface InviteDraftRow {
  /** Local key so rows keep their identity across add/remove. */
  key: string;
  /** Workspace user id, or null until the picker is used. */
  memberId: string | null;
  /** Project role number the row grants. */
  role: number;
}

/** A workspace colleague the invite picker can offer. */
export interface InviteCandidate {
  userId: string;
  /** Workspace role number; the row's cap and default follow it. */
  workspaceRole: number;
  fullName: string;
  displayName: string;
  avatarUrl: string | null;
}

/** Inline message for an invite row that names nobody. */
export const INVITE_MEMBER_REQUIRED = "Select a co-worker";

/** Roles in the order the menus list them: lowest standing first. */
const ROLES_LOW_TO_HIGH: readonly MemberRoleOption[] = [...MEMBER_ROLE_OPTIONS].reverse();

/** The role word a stored number reads as. */
export function roleWord(role: number): string {
  return MEMBER_ROLE_OPTIONS.find((option) => option.role === role)?.label ?? "Guest";
}

/** The role number behind a menu label; null for unknown labels. */
export function roleNumber(label: string): number | null {
  return MEMBER_ROLE_OPTIONS.find((option) => option.label === label)?.role ?? null;
}

/**
 * The project role a workspace role enters at: admins join as admins,
 * members as members, guests as guests.
 */
export function roleForWorkspaceRole(workspaceRole: number): number {
  if (workspaceRole >= ROLE_ADMIN) return ROLE_ADMIN;
  if (workspaceRole >= ROLE_MEMBER) return ROLE_MEMBER;
  return ROLE_GUEST;
}

/**
 * The roles a row may grant (PSET-022 "offered roles follow the
 * co-worker's workspace role"): a workspace guest only ever enters as a
 * guest, a workspace admin at admin level, and a workspace member gets
 * every role. Lowest standing first, matching the menu order.
 */
export function rolesForWorkspaceRole(workspaceRole: number): MemberRoleOption[] {
  if (workspaceRole >= ROLE_ADMIN) return ROLES_LOW_TO_HIGH.filter((option) => option.role >= ROLE_ADMIN);
  if (workspaceRole >= ROLE_MEMBER) return [...ROLES_LOW_TO_HIGH];
  return ROLES_LOW_TO_HIGH.filter((option) => option.role <= ROLE_GUEST);
}

/**
 * The roles a row offers its editor: a workspace guest is capped at
 * Guest, everyone else gets every role. The cap follows the workspace
 * role only — a demoted project guest still offers Member and Admin so
 * the grant restores (PSET-023, NEWFRONT-606).
 */
export function rolesForTarget(workspaceRole: number): MemberRoleOption[] {
  if (workspaceRole <= ROLE_GUEST) return ROLES_LOW_TO_HIGH.filter((option) => option.role === ROLE_GUEST);
  return [...ROLES_LOW_TO_HIGH];
}

function joinedName(entry: WorkspaceMember): string {
  return `${entry.member.first_name} ${entry.member.last_name}`.trim();
}

function candidateName(entry: WorkspaceMember): { fullName: string; displayName: string } {
  const fullName = joinedName(entry);
  const displayName = entry.member.display_name === "" ? fullName : entry.member.display_name;
  return { fullName: fullName === "" ? displayName : fullName, displayName };
}

/**
 * Workspace colleagues who do not already hold an active membership in
 * this project (PSET-022 "current members are excluded"), bots aside.
 * Candidates keep the directory order.
 */
export function inviteCandidates(directory: WorkspaceMemberList, memberships: ProjectMemberList): InviteCandidate[] {
  const taken = new Set(memberships.map((membership) => membership.member));
  const candidates: InviteCandidate[] = [];
  for (const entry of directory) {
    if (entry.member.is_bot === true) continue;
    if (taken.has(entry.member.id)) continue;
    const names = candidateName(entry);
    candidates.push({
      userId: entry.member.id,
      workspaceRole: entry.role,
      fullName: names.fullName,
      displayName: names.displayName,
      avatarUrl: entry.member.avatar_url,
    });
  }
  return candidates;
}

let inviteRowCounter = 0;

/** A fresh, empty invite row. */
export function createInviteDraftRow(): InviteDraftRow {
  inviteRowCounter += 1;
  return { key: `invite-draft-${inviteRowCounter}`, memberId: null, role: ROLE_GUEST };
}

/** Append one empty row to the dialog. */
export function appendInviteDraftRow(rows: readonly InviteDraftRow[]): InviteDraftRow[] {
  return [...rows, createInviteDraftRow()];
}

/** Drop a row; the dialog keeps at least one row to fill in. */
export function removeInviteDraftRow(rows: readonly InviteDraftRow[], key: string): InviteDraftRow[] {
  const kept = rows.filter((row) => row.key !== key);
  return kept.length === 0 ? [createInviteDraftRow()] : kept;
}

/**
 * Pick a co-worker for a row: the row role resets to that person's
 * workspace role, and the offered roles narrow to what that role allows.
 */
export function pickInviteDraftMember(
  rows: readonly InviteDraftRow[],
  key: string,
  candidate: InviteCandidate
): InviteDraftRow[] {
  return rows.map((row) =>
    row.key === key ? { ...row, memberId: candidate.userId, role: roleForWorkspaceRole(candidate.workspaceRole) } : row
  );
}

/** Change one row's role. */
export function setInviteDraftRole(rows: readonly InviteDraftRow[], key: string, role: number): InviteDraftRow[] {
  return rows.map((row) => (row.key === key ? { ...row, role } : row));
}

/** Inline messages keyed by row key; a key present means the row blocks submit. */
export function validateInviteDraftRows(rows: readonly InviteDraftRow[]): Record<string, string> {
  const errors: Record<string, string> = {};
  for (const row of rows) {
    if (row.memberId === null || row.memberId === "") errors[row.key] = INVITE_MEMBER_REQUIRED;
  }
  return errors;
}

/** The members array the bulk-add endpoint takes, in row order. */
export function inviteDraftPayload(rows: readonly InviteDraftRow[]): { member_id: string; role: number }[] {
  return rows.map((row) => ({ member_id: row.memberId ?? "", role: row.role }));
}

/** Identity facts needed to decide one role cell's editability. */
export interface MemberEditor {
  userId: string | null;
  workspaceRole: number | null;
  projectRole: number | null;
}

/** One row's identity for the editability rule. */
export interface MemberTarget {
  userId: string;
  /** The target's workspace role; a workspace admin target is protected. */
  workspaceRole: number | null;
}

/**
 * Whether the role cell is an editor (PSET-023; pin NEWFRONT-401). A
 * workspace admin sees the editor on their own row and on everyone who is
 * not another workspace admin; a project admin sees it on others who are
 * neither themselves nor a workspace admin; everyone else reads plain text.
 * The write may still be refused by the server — the display rule and the
 * server rule are pinned as different things.
 */
export function canEditMemberRole(editor: MemberEditor, target: MemberTarget): boolean {
  const workspaceAdmin = editor.workspaceRole !== null && editor.workspaceRole >= ROLE_ADMIN;
  const projectAdmin = editor.projectRole !== null && editor.projectRole >= ROLE_ADMIN;
  const targetIsWorkspaceAdmin = target.workspaceRole !== null && target.workspaceRole >= ROLE_ADMIN;
  if (workspaceAdmin) return !targetIsWorkspaceAdmin || target.userId === editor.userId;
  if (projectAdmin) return target.userId !== editor.userId && !targetIsWorkspaceAdmin;
  return false;
}

/** Whether the row menu shows Leave (own row) rather than Remove (admins). */
export type MemberRowAction = "leave" | "remove" | "none";

export function memberRowAction(editor: MemberEditor, target: MemberTarget): MemberRowAction {
  if (editor.userId !== null && target.userId === editor.userId) return "leave";
  if (editor.projectRole !== null && editor.projectRole >= ROLE_ADMIN) return "remove";
  if (editor.workspaceRole !== null && editor.workspaceRole >= ROLE_ADMIN) return "remove";
  return "none";
}

/** A member a defaults picker may offer (project members who are not guests). */
export interface DefaultMemberOption {
  userId: string;
  label: string;
}

/** Lead and assignee choices: current project members above guest standing. */
export function defaultMemberOptions(rows: readonly MemberRosterRow[]): DefaultMemberOption[] {
  return rows.filter((row) => row.role > ROLE_GUEST).map((row) => ({ userId: row.userId, label: row.displayName }));
}

/** Menu label that clears a defaults slot. */
export const DEFAULTS_NONE_LABEL = "None";

/** Label a defaults picker shows for the stored user id. */
export function defaultSlotLabel(options: readonly DefaultMemberOption[], userId: string | null): string {
  if (userId === null) return DEFAULTS_NONE_LABEL;
  return options.find((option) => option.userId === userId)?.label ?? DEFAULTS_NONE_LABEL;
}
