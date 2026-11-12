// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row hover menu (parity/specs/workspace-settings.md §8/§13, WSET-013).
// Active rows offer Leave on the actor's own row and Remove to an admin
// on anybody else's; suspended rows offer nothing and never mount the
// trigger. The menu asks the caller to open the shared confirmation.

import { IconButton, Menu } from "@pidash/kit";
import { LogOut, MoreHorizontal, Trash2 } from "lucide-react";
import * as React from "react";

import type { WorkspaceRosterMember } from "@pidash/api-client";

import { memberMenuAction } from "./memberRules.js";

export interface MemberActionsProps {
  row: WorkspaceRosterMember;
  actorUserId: string | null;
  isAdmin: boolean;
  onRemove: (row: WorkspaceRosterMember) => void;
  onLeave: () => void;
}

export function MemberActions({
  row,
  actorUserId,
  isAdmin,
  onRemove,
  onLeave,
}: MemberActionsProps): React.ReactElement | null {
  const action = memberMenuAction(row, actorUserId, isAdmin);
  if (action === null) return null;
  return (
    <Menu
      trigger={
        <IconButton label={`Actions for ${row.member.email}`}>
          <MoreHorizontal size={16} strokeWidth={1.5} />
        </IconButton>
      }
      items={
        action === "leave"
          ? [
              {
                label: "Leave",
                icon: <LogOut size={14} strokeWidth={1.5} aria-hidden="true" />,
                onSelect: onLeave,
              },
            ]
          : [
              {
                label: "Remove",
                icon: <Trash2 size={14} strokeWidth={1.5} aria-hidden="true" />,
                onSelect: () => onRemove(row),
              },
            ]
      }
    />
  );
}
