// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member account-type control (parity/specs/workspace-settings.md §12,
// WSET-012). Admins see it on every active row but their own; it offers
// the roles at or below the actor's own role. The trigger always reads
// the stored role: a successful write refreshes the row, and a refused
// write leaves the stored role showing, so no refused role ever paints.

import { Button, Combobox } from "@pidash/kit";
import * as React from "react";

import type { WorkspaceRosterMember } from "@pidash/api-client";

import { assignableRoles, roleLabelFor, roleValueForLabel } from "./roles.js";

export interface MemberRoleControlProps {
  row: WorkspaceRosterMember;
  actorRole: number | null;
  pending: boolean;
  /** The row whose role changed and the chosen role. */
  onChange: (row: WorkspaceRosterMember, role: number) => void;
}

export function MemberRoleControl({ row, actorRole, pending, onChange }: MemberRoleControlProps): React.ReactElement {
  const role = row.role;
  const label = roleLabelFor(role);

  return (
    <Combobox
      trigger={
        <Button variant="ghost" size="small" aria-label={`Role for ${row.member.email}`} disabled={pending}>
          {label}
        </Button>
      }
      options={assignableRoles(actorRole).map((entry) => ({ id: entry.label, label: entry.label }))}
      value={label}
      onSelect={(next) => {
        const value = roleValueForLabel(actorRole, next);
        if (value === null || value === role) return;
        onChange(row, value);
      }}
      listLabel={`Role for ${row.member.email}`}
      searchEnabled={false}
      emptyText="No roles available"
      disabled={pending}
    />
  );
}
