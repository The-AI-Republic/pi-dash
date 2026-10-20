// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members settings page (parity/specs/workspace-settings.md §7–§14,
// WSET-008–015, 042). The settings shell supplies the frame, gate and tab
// title; this page renders the section header (count chip, persistent
// search, role filter, admin-only Add), the roster table, the skeleton
// until both reads resolve, the inline no-match note, and — for admins —
// the pending-invitations group. Row menus and the account-type control
// carry the role change, removal and self-leave flows through one shared
// confirmation.
//
// New-FRONT-538 owns the manage half: invitations, the pending group, role
// changes, removal/leave and the absence of a billing entry (§14).

import { Badge, Button, cn } from "@pidash/kit";
import { Plus, Search } from "lucide-react";
import * as React from "react";

import type { WorkspaceInvitation } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { useMe, usePermissions, useWorkspaces } from "../../../core/session/index.js";
import { notifySuccess, resolveFileUrl } from "../../../shared/shell/index.js";
import { usePendingInvitations, useWorkspaceRoster } from "./api.js";
import { InviteDialog } from "./InviteDialog.js";
import { copyInvitationLink } from "./invitations.js";
import type { RemoveTarget } from "./memberRules.js";
import { MembersFilters } from "./MembersFilters.js";
import { MembersSkeleton } from "./MembersSkeleton.js";
import { MembersTable } from "./MembersTable.js";
import { PendingInvitations } from "./PendingInvitations.js";
import { RemoveTargetDialog } from "./RemoveTargetDialog.js";
import {
  useChangeInvitationRole,
  useChangeMemberRole,
  useLeaveWorkspace,
  useRemoveMember,
  useSendInvitations,
  useWithdrawInvitation,
} from "./mutations.js";
import {
  applyRosterFilters,
  EMPTY_ROSTER_FILTERS,
  hasMatchingInvitation,
  rosterCount,
  type RosterFilters,
  type RosterRoleFilter,
} from "./roster.js";

/** Inline note when the filters exclude every member (§9). */
export const MEMBERS_NO_MATCH_TEXT = "No matching members";
/** Read failure notice for the two roster reads. */
export const MEMBERS_LOAD_FAILED_TEXT = "Couldn't load the members.";
export const MEMBERS_RETRY_LABEL = "Try again";
/** Admin-only invite entry (§10). */
export const MEMBERS_ADD_LABEL = "Add member";
/** Copy confirmation for an invitation link (§11). */
export const INVITE_LINK_COPIED_TITLE = "Link copied!";

export interface MembersPageProps {
  workspaceSlug: string;
  /** Navigation seam for the self-leave landing; the shell supplies it. */
  onLeaveWorkspace?: (() => void) | undefined;
}

export function MembersPage({ workspaceSlug, onLeaveWorkspace }: MembersPageProps): React.ReactElement {
  const members = useWorkspaceRoster(workspaceSlug);
  const invitations = usePendingInvitations(workspaceSlug);
  const permissions = usePermissions(workspaceSlug);
  const me = useMe();
  const workspaces = useWorkspaces();
  const [filters, setFilters] = React.useState<RosterFilters>(EMPTY_ROSTER_FILTERS);
  const [inviteOpen, setInviteOpen] = React.useState(false);
  const [confirm, setConfirm] = React.useState<RemoveTarget | null>(null);

  const sendInvitations = useSendInvitations(workspaceSlug);
  const changeInviteRole = useChangeInvitationRole(workspaceSlug);
  const withdrawInvitation = useWithdrawInvitation(workspaceSlug);
  const changeMemberRole = useChangeMemberRole(workspaceSlug);
  const removeMember = useRemoveMember(workspaceSlug);
  const leave = useLeaveWorkspace(workspaceSlug);

  const baseUrl = getAppClient().baseUrl;
  const loading = members.isPending || invitations.isPending;
  const failed = members.isError || invitations.isError;
  const isAdmin = permissions.isAdmin;

  const allMembers = members.data ?? [];
  const allInvitations = invitations.data ?? [];
  const visible = React.useMemo(() => applyRosterFilters(allMembers, filters), [allMembers, filters]);
  // §9: the note waits until the search excludes every member *and* every
  // invitation. The role filter narrows members only, so it never decides
  // whether an invitation still matches.
  const noMatch = !loading && !failed && visible.length === 0 && !hasMatchingInvitation(allInvitations, filters.search);

  const search = (value: string): void => {
    setFilters((current) => ({ ...current, search: value }));
  };
  const setRoles = (roles: RosterRoleFilter[]): void => {
    setFilters((current) => ({ ...current, roles }));
  };
  const setOrder = (order: RosterFilters["order"]): void => {
    setFilters((current) => ({ ...current, order }));
  };
  const retry = (): void => {
    void members.refetch();
    void invitations.refetch();
  };

  const actorUserId = me.data?.id ?? null;
  const workspaceRole = workspaces.data?.find((entry) => entry.slug === workspaceSlug)?.role ?? null;

  function copyInviteLink(invitation: WorkspaceInvitation): void {
    // §11: a refused clipboard stays silent; a landed copy confirms.
    copyInvitationLink(invitation, window.location.origin, navigator.clipboard, () =>
      notifySuccess(INVITE_LINK_COPIED_TITLE)
    );
  }

  function confirmTarget(target: RemoveTarget): void {
    if (target.kind === "invitation") {
      withdrawInvitation.mutate({ invitationId: target.invitation.id }, { onSettled: () => setConfirm(null) });
      return;
    }
    if (target.kind === "member") {
      removeMember.mutate({ membershipId: target.row.id }, { onSettled: () => setConfirm(null) });
      return;
    }
    leave.mutate(undefined, {
      onSettled: () => setConfirm(null),
      onSuccess: () => onLeaveWorkspace?.(),
    });
  }

  const confirmPending = withdrawInvitation.isPending || removeMember.isPending || leave.isPending;

  // Tests pin the count rule without a browser; the page never recomputes it.
  const count = rosterCount(allMembers);

  return (
    <section aria-label="Members" data-testid="workspace-settings-members" className="flex flex-col gap-(--space-4)">
      <div className="flex flex-wrap items-center justify-between gap-(--space-3)">
        <h2 className="text-h3 flex items-center gap-(--space-2) text-(--text)">
          Members
          {count > 0 ? (
            <span data-testid="members-count" aria-label={`${count} members`}>
              <Badge>{count}</Badge>
            </span>
          ) : null}
        </h2>
        <div className="flex flex-wrap items-center gap-(--space-2)">
          <label className="relative inline-flex items-center">
            <span className="sr-only">Search members</span>
            <Search
              size={14}
              strokeWidth={1.5}
              aria-hidden="true"
              className="pointer-events-none absolute left-(--space-3) text-(--text-muted)"
            />
            <input
              type="text"
              autoFocus
              value={filters.search}
              placeholder="Search..."
              onChange={(event) => search(event.target.value)}
              className={cn(
                "text-body h-(--control-height) w-56 rounded-(--radius-control) border border-(--border) bg-(--bg) pr-(--space-3) pl-(--space-8) text-(--text) placeholder:text-(--text-muted)",
                "focus-visible:ring-2 focus-visible:ring-(--accent) focus-visible:ring-offset-1 focus-visible:ring-offset-(--bg) focus-visible:outline-none"
              )}
            />
          </label>
          <MembersFilters roles={filters.roles} onChange={setRoles} />
          {isAdmin ? (
            <Button variant="primary" onClick={() => setInviteOpen(true)}>
              <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
              {MEMBERS_ADD_LABEL}
            </Button>
          ) : null}
        </div>
      </div>

      {loading ? <MembersSkeleton /> : null}

      {!loading && failed ? (
        <div
          role="alert"
          data-testid="members-load-failed"
          className="text-body flex items-center gap-(--space-3) rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-3) text-(--text-muted)"
        >
          {MEMBERS_LOAD_FAILED_TEXT}
          <Button variant="secondary" size="small" onClick={retry}>
            {MEMBERS_RETRY_LABEL}
          </Button>
        </div>
      ) : null}

      {noMatch ? (
        <p data-testid="members-no-match" className="text-body py-(--space-12) text-center text-(--text-muted)">
          {MEMBERS_NO_MATCH_TEXT}
        </p>
      ) : null}

      {!loading && !failed && !noMatch ? (
        <MembersTable
          workspaceSlug={workspaceSlug}
          rows={visible}
          order={filters.order}
          onOrderChange={setOrder}
          avatarUrl={(path) => resolveFileUrl(path, baseUrl)}
          actorUserId={actorUserId}
          actorRole={workspaceRole}
          rolePending={changeMemberRole.isPending}
          onRoleChange={(row, role) => {
            // §12: no optimistic write on the roster; the settled read keeps
            // the old role when the server refuses.
            changeMemberRole.mutate({ membershipId: row.id, role });
          }}
          onRemove={(row) => setConfirm({ kind: "member", row })}
          onLeave={() => setConfirm({ kind: "self" })}
        />
      ) : null}

      {!loading && !failed && isAdmin ? (
        <PendingInvitations
          invitations={allInvitations}
          search={filters.search}
          actorRole={workspaceRole}
          pending={changeInviteRole.isPending}
          onRoleChange={(invitationId, role) => changeInviteRole.mutate({ invitationId, role })}
          onCopyLink={copyInviteLink}
          onRemove={(invitation) => setConfirm({ kind: "invitation", invitation })}
        />
      ) : null}

      <InviteDialog
        open={inviteOpen}
        actorRole={workspaceRole}
        pending={sendInvitations.isPending}
        onOpenChange={setInviteOpen}
        onSubmit={(emails) => {
          // §10: success closes the dialog; the mutation toasts on both
          // outcomes and a failure leaves the rows untouched.
          sendInvitations.mutate({ emails }, { onSuccess: () => setInviteOpen(false) });
        }}
      />

      {confirm === null ? null : (
        <RemoveTargetDialog
          target={confirm}
          pending={confirmPending}
          onCancel={() => setConfirm(null)}
          onConfirm={() => confirmTarget(confirm)}
        />
      )}
    </section>
  );
}
