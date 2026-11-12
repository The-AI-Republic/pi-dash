// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pending-invitation role control (parity/specs/workspace-settings.md
// §11, WSET-010). Offers only the roles the actor may hand out; picking
// one sends the role write and the row settles on the refreshed read, so
// a refused write leaves the stored role showing. The callers own the
// notice; this control only asks for the change.

import { Button, Combobox } from "@pidash/kit";
import * as React from "react";

import type { WorkspaceInvitation } from "@pidash/api-client";

import { assignableRoles, roleLabelFor, roleValueForLabel } from "./roles.js";

export interface InvitationRoleControlProps {
  invitation: WorkspaceInvitation;
  actorRole: number | null;
  disabled: boolean;
  onChange: (invitationId: string, role: number) => void;
}

export function InvitationRoleControl({
  invitation,
  actorRole,
  disabled,
  onChange,
}: InvitationRoleControlProps): React.ReactElement {
  const role = invitation.role;
  const label = roleLabelFor(role);
  const options = assignableRoles(actorRole);

  return (
    <Combobox
      trigger={
        <Button variant="ghost" size="small" aria-label={`Role for ${invitation.email}`} disabled={disabled}>
          {label}
        </Button>
      }
      options={options.map((entry) => ({ id: entry.label, label: entry.label }))}
      value={label}
      onSelect={(next) => {
        const value = roleValueForLabel(actorRole, next);
        if (value === null || value === role) return;
        onChange(invitation.id, value);
      }}
      listLabel={`Role for ${invitation.email}`}
      searchEnabled={false}
      emptyText="No roles available"
      disabled={disabled}
    />
  );
}
