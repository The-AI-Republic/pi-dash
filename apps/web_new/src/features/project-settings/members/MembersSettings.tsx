// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members tab body (parity/specs/project-settings-members.md §"Member
// table", §"Search, filter and sort", §"Add members", §"Inline role
// changes", §"Remove and leave-self", §"Project defaults", §"Loading and
// failure states"; PSET-018–027). Reads the memberships, the workspace
// directory and the project row, shows a skeleton until the roster settles,
// then the project defaults, the search box, the role filter and the table.
// Admins get the add control; a role cell opens an editor where the rule
// allows it; every row's name cell reveals the remove-or-leave menu.

import { Button, cn } from "@pidash/kit";
import { Search } from "lucide-react";
import * as React from "react";

import type { WorkspaceMemberList } from "@pidash/api-client";

import { useNavigate, useRouterState } from "@tanstack/react-router";

import { getAppClient } from "../../../core/api/client.js";
import { ROLE_ADMIN } from "../../../core/session/permissions.js";
import { isAdminStanding } from "../tabs.js";
import { resolveFileUrl } from "../../../shared/shell/index.js";
import {
  notifyMembersLoadFailure,
  useAddProjectMembers,
  useLeaveProject,
  useMemberEditor,
  useProjectDefaults,
  useProjectMemberships,
  useRemoveProjectMember,
  useSaveProjectDefaults,
  useUpdateProjectMemberRole,
  useWorkspaceDirectory,
} from "./api.js";
import { useMemberFilters } from "./filters.js";
import { InviteMembersDialog } from "./InviteMembersDialog.js";
import { MemberDefaults } from "./MemberDefaults.js";
import { MemberFilters } from "./MemberFilters.js";
import { MemberSkeleton } from "./MemberSkeleton.js";
import { MemberTable, type MemberRowControls } from "./MemberTable.js";
import {
  applyMemberRosterFilters,
  buildMemberRoster,
  type MemberRosterFilters,
  type MemberRoleKey,
  type MemberRosterRow,
} from "./roster.js";
import {
  canEditMemberRole,
  defaultMemberOptions,
  memberRowAction,
  rolesForTarget,
  type MemberEditor,
  type MemberRowAction,
} from "./writes.js";

/** Centered note when search and role filters exclude every row. */
export const MEMBERS_NO_MATCH_TEXT = "No matching members";
/** Label of the admin-only control that opens the invite dialog. */
export const ADD_MEMBER_LABEL = "Add member";

export interface MembersSettingsProps {
  workspaceSlug: string;
  projectId: string;
  /** Resolves a stored avatar path; defaults to the app client's origin. */
  resolveAvatar?: (path: string | null) => string | null;
}

/**
 * Fire the failure toast once per failed resolution: the two reads are
 * independent, and the surface keeps its skeleton or table either way.
 */
function useReadFailureNotice(errors: readonly (unknown | null)[]): void {
  const notified = React.useRef(new Set<unknown>());
  React.useEffect(() => {
    for (const error of errors) {
      if (error === null || notified.current.has(error)) continue;
      notified.current.add(error);
      notifyMembersLoadFailure(error);
    }
  }, [errors]);
}

/** Workspace role per user id, for the editability rule and the guest cap. */
function workspaceRoles(directory: WorkspaceMemberList | undefined): Map<string, number> {
  const roles = new Map<string, number>();
  for (const entry of directory ?? []) roles.set(entry.member.id, entry.role);
  return roles;
}

export function MembersSettings({ workspaceSlug, projectId, resolveAvatar }: MembersSettingsProps): React.ReactElement {
  const memberships = useProjectMemberships(workspaceSlug, projectId);
  const directory = useWorkspaceDirectory(workspaceSlug);
  const project = useProjectDefaults(workspaceSlug, projectId);
  const editorState = useMemberEditor(workspaceSlug, projectId);
  const [persisted, setPersisted] = useMemberFilters(projectId);
  const [search, setSearch] = React.useState("");
  const [inviteOpen, setInviteOpen] = React.useState(false);
  const [roleOverrides, setRoleOverrides] = React.useState<Record<string, number>>({});
  const [pendingMembershipId, setPendingMembershipId] = React.useState<string | null>(null);
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (state) => state.location.pathname });

  const addMembers = useAddProjectMembers(workspaceSlug, projectId);
  const saveRole = useUpdateProjectMemberRole(workspaceSlug, projectId, ({ memberId }) => {
    setRoleOverrides((current) => {
      if (current[memberId] === undefined) return current;
      const next = { ...current };
      delete next[memberId];
      return next;
    });
  });
  const removeMember = useRemoveProjectMember(workspaceSlug, projectId);
  const leaveProject = useLeaveProject(workspaceSlug, projectId);
  const saveDefaults = useSaveProjectDefaults(workspaceSlug, projectId);

  useReadFailureNotice([memberships.error ?? null, directory.error ?? null]);

  const loading = memberships.isPending || directory.isPending;
  // A refused read keeps the skeleton rather than claiming "no matches":
  // the toast explains the failure and the surface stays put (PSET-027).
  const unavailable =
    (memberships.isError && memberships.data === undefined) || (directory.isError && directory.data === undefined);
  const roster = React.useMemo(
    () =>
      memberships.data === undefined || directory.data === undefined
        ? []
        : buildMemberRoster(memberships.data, directory.data),
    [memberships.data, directory.data]
  );
  const filters: MemberRosterFilters = React.useMemo(() => ({ ...persisted, search }), [persisted, search]);
  const visible = React.useMemo(() => applyMemberRosterFilters(roster, filters), [roster, filters]);
  const rolesByUser = React.useMemo(() => workspaceRoles(directory.data), [directory.data]);

  const resolveAvatarFile = React.useCallback(
    (path: string | null) =>
      resolveAvatar === undefined ? resolveFileUrl(path, getAppClient().baseUrl) : resolveAvatar(path),
    [resolveAvatar]
  );

  const setRoles = (roles: MemberRoleKey[]): void => {
    setPersisted({ ...persisted, roles });
  };

  const editor: MemberEditor = {
    userId: editorState.userId,
    workspaceRole: editorState.workspaceRole,
    projectRole: editorState.projectRole,
  };
  const canAdd = isAdminStanding(editorState.projectRole) || (editorState.workspaceRole ?? 0) >= ROLE_ADMIN;
  const canEditDefaults = canAdd;
  const defaults = React.useMemo(() => defaultMemberOptions(roster), [roster]);

  function runRowAction(row: MemberRosterRow, action: Exclude<MemberRowAction, "none">): void {
    setPendingMembershipId(row.membershipId);
    if (action === "leave") {
      leaveProject.mutate(undefined, {
        onSuccess: () => {
          // The membership is gone: land on the workspace project list.
          setPendingMembershipId(null);
          void navigate({ to: "/$ws/projects", params: { ws: workspaceSlug } });
        },
        onSettled: () => setPendingMembershipId(null),
      });
      return;
    }
    removeMember.mutate({ memberId: row.membershipId }, { onSettled: () => setPendingMembershipId(null) });
  }

  const controls: MemberRowControls = {
    canEditRole: (row) =>
      canEditMemberRole(editor, { userId: row.userId, workspaceRole: rolesByUser.get(row.userId) ?? null }),
    roleOptions: (row) => rolesForTarget(rolesByUser.get(row.userId) ?? 0),
    roleOverride: (row) => roleOverrides[row.membershipId] ?? null,
    onPickRole: (row, role) => {
      setRoleOverrides((current) => ({ ...current, [row.membershipId]: role }));
      saveRole.mutate({ memberId: row.membershipId, role });
    },
    onRowAction: runRowAction,
    action: (row) =>
      memberRowAction(editor, { userId: row.userId, workspaceRole: rolesByUser.get(row.userId) ?? null }),
    pendingMembershipId,
  };

  // The invite dialog belongs to this address: leaving the tab closes it
  // rather than letting it linger over another page.
  React.useEffect(() => {
    setInviteOpen(false);
  }, [pathname]);

  return (
    <section
      aria-label="Members settings"
      data-testid="project-settings-members"
      data-slice="NEWFRONT-579"
      className="flex flex-col gap-(--space-6)"
    >
      <MemberDefaults
        project={project.data}
        members={defaults}
        canEdit={canEditDefaults}
        pending={saveDefaults.isPending}
        onSave={(patch) => saveDefaults.mutate(patch)}
      />
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <span className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-3) py-(--space-1)">
          <Search size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
          <input
            aria-label="Search members"
            placeholder="Search members"
            value={search}
            onChange={(event) => setSearch(event.currentTarget.value)}
            className="text-body w-56 border-none bg-transparent text-(--text) placeholder:text-(--text-muted) focus:outline-none"
          />
        </span>
        <span className="flex items-center gap-(--space-2)">
          <MemberFilters roles={persisted.roles} onChange={setRoles} />
          {canAdd ? (
            <Button variant="primary" onClick={() => setInviteOpen(true)}>
              {ADD_MEMBER_LABEL}
            </Button>
          ) : null}
        </span>
      </div>
      {loading || unavailable ? (
        <MemberSkeleton />
      ) : visible.length === 0 ? (
        <p data-testid="members-no-match" className={cn("text-body py-(--space-12) text-center text-(--text-muted)")}>
          {MEMBERS_NO_MATCH_TEXT}
        </p>
      ) : (
        <MemberTable
          workspaceSlug={workspaceSlug}
          rows={visible}
          sort={persisted.sort}
          onSortChange={(sort) => setPersisted({ ...persisted, sort })}
          resolveAvatar={resolveAvatarFile}
          controls={controls}
        />
      )}
      <InviteMembersDialog
        open={inviteOpen}
        directory={directory.data ?? []}
        memberships={memberships.data ?? []}
        pending={addMembers.isPending}
        onOpenChange={setInviteOpen}
        onSubmit={(members) => {
          addMembers.mutate({ members }, { onSuccess: () => setInviteOpen(false) });
        }}
      />
    </section>
  );
}
