// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// One roster row (parity/specs/workspace-settings.md §8/§12/§13,
// WSET-011/012/013). The identity cell pairs the avatar with the full name
// and links to the member profile, then a trailing cell carries the row
// menu. Suspended rows dim, drop the link, the trailing facts, the role
// control and the menu, and read "Suspended" where the role would be.
// Admins get an account-type control on every active row but their own;
// authentication hides when the account never signed in; the joining date
// is formatted for display only.

import { Avatar, cn } from "@pidash/kit";
import { UserX } from "lucide-react";
import * as React from "react";

import type { WorkspaceRosterMember } from "@pidash/api-client";

import { ROLE_ADMIN } from "../../../core/session/index.js";
import { MemberActions } from "./MemberActions.js";
import { MemberRoleControl } from "./MemberRoleControl.js";
import { memberRoleEditable } from "./memberRules.js";
import {
  authMediumLabel,
  isSuspended,
  roleLabel,
  rosterAvatarInitial,
  rosterFullName,
  rosterIdentityName,
} from "./roster.js";

export interface MemberRowProps {
  workspaceSlug: string;
  row: WorkspaceRosterMember;
  /** Absolute or API-relative avatar URL already resolved for rendering. */
  avatarUrl: string | null;
  actorUserId: string | null;
  actorRole: number | null;
  rolePending: boolean;
  onRoleChange: (row: WorkspaceRosterMember, role: number) => void;
  onRemove: (row: WorkspaceRosterMember) => void;
  onLeave: () => void;
}

const JOINING_FORMAT = new Intl.DateTimeFormat("en-US", { year: "numeric", month: "short", day: "numeric" });

/** Joining date as the roster shows it; empty when unparseable. */
export function formatJoiningDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return JOINING_FORMAT.format(date);
}

export function MemberRow({
  workspaceSlug,
  row,
  avatarUrl,
  actorUserId,
  actorRole,
  rolePending,
  onRoleChange,
  onRemove,
  onLeave,
}: MemberRowProps): React.ReactElement {
  const suspended = isSuspended(row);
  const name = rosterIdentityName(row.member);
  const initial = rosterAvatarInitial(row.member);
  const profileHref = `/${workspaceSlug}/profile/${row.member.id}/`;
  const auth = suspended ? null : authMediumLabel(row.member.last_login_medium);
  const joined = suspended ? "" : formatJoiningDate(row.created_at);
  const isAdmin = actorRole !== null && actorRole >= ROLE_ADMIN;
  const canChangeRole = memberRoleEditable(row, actorUserId, isAdmin);

  return (
    <tr
      data-testid="member-row"
      data-email={row.member.email}
      data-active={suspended ? "false" : "true"}
      className={cn("group border-b border-(--border) last:border-b-0", suspended && "text-(--text-muted)")}
    >
      <td className="py-(--space-2) pr-(--space-4)">
        <span className="flex items-center gap-(--space-2)">
          {suspended ? (
            <span
              data-avatar-kind="suspended"
              className="inline-flex size-8 items-center justify-center rounded-full bg-(--subtle)"
            >
              <UserX size={16} strokeWidth={1.5} aria-label="Suspended" className="text-(--text-muted)" />
            </span>
          ) : (
            <a
              href={profileHref}
              aria-label={`Open ${name}'s profile`}
              className="inline-flex rounded-full focus-visible:ring-2 focus-visible:ring-(--accent)"
            >
              {avatarUrl === null ? (
                <span data-avatar-kind="initial" className="inline-flex">
                  <Avatar name={name} initial={initial} size="medium" />
                </span>
              ) : (
                <span data-avatar-kind="image" className="inline-flex">
                  <Avatar name={name} initial={initial} src={avatarUrl} size="medium" />
                </span>
              )}
            </a>
          )}
          <span data-testid="member-full-name" className="truncate">
            {rosterFullName(row.member)}
          </span>
        </span>
      </td>
      <td className="py-(--space-2) pr-(--space-4)">{row.member.display_name}</td>
      <td className="py-(--space-2) pr-(--space-4)">{row.member.email}</td>
      <td className="py-(--space-2) pr-(--space-4)">
        {suspended ? (
          "Suspended"
        ) : canChangeRole ? (
          <MemberRoleControl row={row} actorRole={actorRole} pending={rolePending} onChange={onRoleChange} />
        ) : (
          roleLabel(row.role)
        )}
      </td>
      <td className="py-(--space-2) pr-(--space-4)">{auth ?? null}</td>
      <td className="py-(--space-2) pr-(--space-4)">
        {joined === "" ? null : <time dateTime={row.created_at}>{joined}</time>}
      </td>
      <td className="py-(--space-2) text-right">
        <MemberActions row={row} actorUserId={actorUserId} isAdmin={isAdmin} onRemove={onRemove} onLeave={onLeave} />
      </td>
    </tr>
  );
}
