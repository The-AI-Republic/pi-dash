// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk invite dialog (parity/specs/workspace-settings.md §10, WSET-009).
// One address-and-role row per invitee, all of them defaulting to Member.
// Add-more appends a row; a row can be dropped once there is more than
// one. A submit with a blank or malformed address paints the message
// under that row and sends nothing. A refused send keeps the dialog and
// its input; a successful one closes, resets and toasts. Escape and a
// backdrop click close it like any sibling settings dialog (§37).

import { Button, Combobox, Dialog, IconButton, Input, cn } from "@pidash/kit";
import { Plus, Trash2 } from "lucide-react";
import * as React from "react";

import type { WorkspaceInvitee } from "@pidash/api-client";

import { assignableRoles, initialInviteRole, roleLabelFor, roleValueForLabel } from "./roles.js";
import {
  appendInviteRow,
  createInviteRow,
  invitePayload,
  removeInviteRow,
  setInviteRowEmail,
  setInviteRowRole,
  validateInviteRows,
  type InviteRow,
} from "./inviteForm.js";

export const INVITE_DIALOG_TITLE = "Invite people to collaborate";
export const INVITE_DIALOG_DESCRIPTION =
  "Send workspace invitations. Each invitee keeps the role chosen beside their address.";

export interface InviteDialogProps {
  open: boolean;
  /** The actor's workspace role; limits every row's role options. */
  actorRole: number | null;
  /** A send is in flight: the dialog holds its input and locks its controls. */
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (emails: WorkspaceInvitee[]) => void;
}

function InviteRolePicker({
  actorRole,
  role,
  pending,
  onPick,
}: {
  actorRole: number | null;
  role: number;
  pending: boolean;
  onPick: (value: number) => void;
}): React.ReactElement {
  const options = assignableRoles(actorRole);
  const current = roleLabelFor(role);
  return (
    <Combobox
      trigger={
        <Button variant="secondary" aria-label="Invitee role" disabled={pending} className="w-28 justify-between">
          {current}
        </Button>
      }
      options={options.map((entry) => ({ id: entry.label, label: entry.label }))}
      value={current}
      onSelect={(label) => {
        const value = roleValueForLabel(actorRole, label);
        if (value !== null) onPick(value);
      }}
      listLabel="Invitee role"
      searchEnabled={false}
      emptyText="No roles available"
      disabled={pending}
    />
  );
}

export function InviteDialog({
  open,
  actorRole,
  pending,
  onOpenChange,
  onSubmit,
}: InviteDialogProps): React.ReactElement {
  const [rows, setRows] = React.useState<InviteRow[]>(() => [createInviteRow(initialInviteRole(actorRole))]);
  const [errors, setErrors] = React.useState<Record<string, string>>({});

  // A fresh dialog per visit: reopening never inherits addresses or errors.
  React.useEffect(() => {
    if (!open) return;
    setRows([createInviteRow(initialInviteRole(actorRole))]);
    setErrors({});
  }, [open, actorRole]);

  function submit(): void {
    // New invites reuse the row's local key in the error map; leaving the
    // dialog open is what lets the user correct a row in place.
    const found = validateInviteRows(rows);
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    onSubmit(invitePayload(rows));
  }

  function updateEmail(key: string, email: string): void {
    setRows((current) => setInviteRowEmail(current, key, email));
    setErrors((current) => {
      if (current[key] === undefined) return current;
      const next = { ...current };
      delete next[key];
      return next;
    });
  }

  return (
    <Dialog
      open={open}
      title={INVITE_DIALOG_TITLE}
      description={INVITE_DIALOG_DESCRIPTION}
      onOpenChange={onOpenChange}
      nonDismissable={pending}
      className="max-w-xl"
      actions={
        <>
          <Button variant="secondary" disabled={pending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} onClick={submit}>
            Send invitations
          </Button>
        </>
      }
    >
      <div data-testid="invite-dialog" className="flex flex-col gap-(--space-4)">
        <div className="flex max-h-80 flex-col gap-(--space-3) overflow-y-auto">
          {rows.map((row, index) => (
            <div
              key={row.key}
              data-testid={`invite-row-${index}`}
              className="flex items-start gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-3)"
            >
              <div className="min-w-0 flex-1">
                <Input
                  label={index === 0 ? "Email" : `Email ${index + 1}`}
                  // The driver finds each row's field by this name; it also
                  // keeps rows distinguishable to assistive tech.
                  name={`emails.${index}.email`}
                  value={row.email}
                  type="email"
                  autoComplete="off"
                  placeholder="name@company.com"
                  onChange={(event) => updateEmail(row.key, event.target.value)}
                  {...(errors[row.key] === undefined ? {} : { error: errors[row.key] })}
                />
              </div>
              <div className="pt-(--space-7)">
                <InviteRolePicker
                  actorRole={actorRole}
                  role={row.role}
                  pending={pending}
                  onPick={(value) => setRows((current) => setInviteRowRole(current, row.key, value))}
                />
              </div>
              <div className="pt-(--space-7)">
                <IconButton
                  label="Remove this invitee"
                  disabled={rows.length === 1 || pending}
                  onClick={() => setRows((current) => removeInviteRow(current, row.key))}
                  className={cn(rows.length === 1 && "invisible")}
                >
                  <Trash2 size={16} strokeWidth={1.5} />
                </IconButton>
              </div>
            </div>
          ))}
        </div>
        <div>
          <Button
            variant="ghost"
            size="small"
            disabled={pending}
            onClick={() => setRows((current) => appendInviteRow(current, initialInviteRole(actorRole)))}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            Add more
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
