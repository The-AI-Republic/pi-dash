// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending invitations (parity/specs/workspace-settings.md §11, WSET-010).
// A collapsible group under the roster, admin-only, listing every pending
// invitation the search leaves. Its heading carries the count of those
// rows; each row shows the address, a pending marker, its role control and
// a menu with Copy link (only when the row carries a link) and Remove.
// The group renders only while at least one invitation matches the search.

import { Avatar, IconButton, Menu } from "@pidash/kit";
import { ChevronDown, ChevronRight, Link2, MoreHorizontal, Trash2 } from "lucide-react";
import * as React from "react";

import type { WorkspaceInvitation } from "@pidash/api-client";

import { invitationInitial, invitationLink, PENDING_GROUP_LABEL, selectPendingInvitations } from "./invitations.js";
import { InvitationRoleControl } from "./InvitationRoleControl.js";

export interface PendingInvitationsProps {
  invitations: readonly WorkspaceInvitation[];
  search: string;
  actorRole: number | null;
  pending: boolean;
  onRoleChange: (invitationId: string, role: number) => void;
  onCopyLink: (invitation: WorkspaceInvitation) => void;
  onRemove: (invitation: WorkspaceInvitation) => void;
}

export function PendingInvitations({
  invitations,
  search,
  actorRole,
  pending,
  onRoleChange,
  onCopyLink,
  onRemove,
}: PendingInvitationsProps): React.ReactElement | null {
  const [expanded, setExpanded] = React.useState(true);
  const matching = React.useMemo(() => selectPendingInvitations(invitations, search), [invitations, search]);

  if (matching.length === 0) return null;
  const origin = window.location.origin;

  return (
    <section
      data-testid="pending-invitations"
      aria-label={PENDING_GROUP_LABEL}
      className="flex flex-col gap-(--space-2)"
    >
      <div className="flex items-center justify-between gap-(--space-2)">
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
          className="text-emphasis flex cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) text-(--text) hover:bg-(--subtle)"
        >
          {expanded ? (
            <ChevronDown size={16} strokeWidth={1.5} aria-hidden="true" />
          ) : (
            <ChevronRight size={16} strokeWidth={1.5} aria-hidden="true" />
          )}
          {PENDING_GROUP_LABEL}
          <span data-testid="pending-invitations-count" className="text-body text-(--text-muted)">
            {matching.length}
          </span>
        </button>
      </div>
      {expanded ? (
        <ul
          aria-label="Pending invitations"
          className="flex flex-col rounded-(--radius-control) border border-(--border)"
        >
          {matching.map((invitation) => {
            const link = invitationLink(invitation, origin);
            return (
              <li
                key={invitation.id}
                data-testid="pending-invitation-row"
                data-email={invitation.email}
                className="flex items-center gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-2) last:border-b-0"
              >
                <Avatar name={invitation.email} initial={invitationInitial(invitation.email)} size="medium" />
                <span className="min-w-0 flex-1 truncate text-(--text)">{invitation.email}</span>
                <span className="text-meta text-(--text-muted)">Awaiting acceptance</span>
                <InvitationRoleControl
                  invitation={invitation}
                  actorRole={actorRole}
                  disabled={pending}
                  onChange={onRoleChange}
                />
                <Menu
                  trigger={
                    <IconButton label={`Actions for ${invitation.email}`}>
                      <MoreHorizontal size={16} strokeWidth={1.5} />
                    </IconButton>
                  }
                  items={[
                    ...(link === null
                      ? []
                      : [
                          {
                            label: "Copy link",
                            icon: <Link2 size={14} strokeWidth={1.5} aria-hidden="true" />,
                            onSelect: () => onCopyLink(invitation),
                          },
                        ]),
                    {
                      label: "Remove",
                      icon: <Trash2 size={14} strokeWidth={1.5} aria-hidden="true" />,
                      onSelect: () => onRemove(invitation),
                    },
                  ]}
                />
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
