// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Multi-row add-members dialog (parity/specs/project-settings-members.md
// §"Add members"; PSET-022). Starts with one row; rows pair a colleague
// picker (workspace members without a project membership) with a role
// menu whose choices follow the colleague's workspace role. A row without
// a colleague paints its own message and blocks the send; one submit sends
// every row in a single bulk-add.

import { Avatar, Button, Combobox, Dialog, IconButton, cn, type ComboboxOption } from "@pidash/kit";
import { Plus, Trash2 } from "lucide-react";
import * as React from "react";

import type { ProjectMemberList, WorkspaceMemberList } from "@pidash/api-client";

import {
  appendInviteDraftRow,
  createInviteDraftRow,
  inviteCandidates,
  inviteDraftPayload,
  pickInviteDraftMember,
  removeInviteDraftRow,
  roleForWorkspaceRole,
  roleWord,
  rolesForWorkspaceRole,
  setInviteDraftRole,
  validateInviteDraftRows,
  type InviteCandidate,
  type InviteDraftRow,
} from "./writes.js";

export const INVITE_DIALOG_TITLE = "Invite members";
export const INVITE_DIALOG_DESCRIPTION =
  "Add colleagues from this workspace. Each row is added with the role beside it.";

export interface InviteMembersDialogProps {
  open: boolean;
  /** Workspace directory; the picker offers the colleagues not yet here. */
  directory: WorkspaceMemberList;
  /** The project's current memberships; those people are not offered. */
  memberships: ProjectMemberList;
  pending: boolean;
  onOpenChange: (open: boolean) => void;
  onSubmit: (rows: { member_id: string; role: number }[]) => void;
}

function candidatesByName(candidates: readonly InviteCandidate[]): InviteCandidate[] {
  return [...candidates].sort((a, b) => a.fullName.toLowerCase().localeCompare(b.fullName.toLowerCase()));
}

function RowMemberPicker({
  index,
  row,
  candidates,
  pending,
  onPick,
}: {
  index: number;
  row: InviteDraftRow;
  candidates: readonly InviteCandidate[];
  pending: boolean;
  onPick: (candidate: InviteCandidate) => void;
}): React.ReactElement {
  const chosen = candidates.find((candidate) => candidate.userId === row.memberId) ?? null;
  const options: ComboboxOption[] = candidatesByName(candidates).map((candidate) => ({
    id: candidate.userId,
    label: candidate.fullName,
    searchText: candidate.displayName,
    icon: <Avatar name={candidate.fullName} size="small" />,
    content: (
      <span data-testid={`invite-candidate-${index}`} className="flex min-w-0 flex-col">
        <span className="truncate">{candidate.fullName}</span>
        <span className="text-caption truncate text-(--text-muted)">{candidate.displayName}</span>
      </span>
    ),
  }));
  return (
    <Combobox
      trigger={
        <Button
          variant="secondary"
          aria-label="Select co-worker"
          aria-haspopup="listbox"
          disabled={pending}
          className={cn("w-56 justify-start", chosen === null && "text-(--text-muted)")}
        >
          {chosen === null ? "Select co-worker" : chosen.fullName}
        </Button>
      }
      options={options}
      value={row.memberId}
      onSelect={(id) => {
        const candidate = candidates.find((entry) => entry.userId === id);
        if (candidate !== undefined) onPick(candidate);
      }}
      listLabel="Workspace colleagues"
      searchLabel="Search colleagues"
      emptyText="Everyone in this workspace is already a member"
      disabled={pending}
      className="w-72"
    />
  );
}

function RowRolePicker({
  index,
  row,
  candidates,
  pending,
  onPick,
}: {
  index: number;
  row: InviteDraftRow;
  candidates: readonly InviteCandidate[];
  pending: boolean;
  onPick: (role: number) => void;
}): React.ReactElement {
  const chosen = candidates.find((candidate) => candidate.userId === row.memberId) ?? null;
  // With nobody picked yet, the row offers the entry-level role only; the
  // freedom arrives with the colleague's workspace role.
  const workspaceRole = chosen?.workspaceRole ?? 0;
  const offered =
    chosen === null
      ? [roleForWorkspaceRole(workspaceRole)]
      : rolesForWorkspaceRole(workspaceRole).map((option) => option.role);
  // A row's role only ever comes from this list (pick resets it, the menu
  // picks from it), so it is always among the offered roles.
  const picked = row.role;
  return (
    <Combobox
      trigger={
        <Button
          variant="secondary"
          aria-label="Member role"
          aria-haspopup="listbox"
          disabled={pending}
          className="w-28 justify-between"
        >
          {roleWord(picked)}
        </Button>
      }
      options={offered.map((role) => ({ id: String(role), label: roleWord(role) }))}
      value={String(picked)}
      onSelect={(id) => onPick(Number(id))}
      listLabel={`Row ${index + 1} role`}
      searchEnabled={false}
      emptyText="No roles available"
      disabled={pending || chosen === null}
    />
  );
}

export function InviteMembersDialog({
  open,
  directory,
  memberships,
  pending,
  onOpenChange,
  onSubmit,
}: InviteMembersDialogProps): React.ReactElement {
  const [rows, setRows] = React.useState<InviteDraftRow[]>(() => [createInviteDraftRow()]);
  const [errors, setErrors] = React.useState<Record<string, string>>({});
  const candidates = React.useMemo(() => inviteCandidates(directory, memberships), [directory, memberships]);

  // Each visit starts clean: one blank row, no messages.
  React.useEffect(() => {
    if (!open) return;
    setRows([createInviteDraftRow()]);
    setErrors({});
  }, [open]);

  function updateRows(next: InviteDraftRow[]): void {
    setRows(next);
    if (Object.keys(errors).length === 0) return;
    setErrors(validateInviteDraftRows(next));
  }

  function submit(): void {
    const found = validateInviteDraftRows(rows);
    setErrors(found);
    if (Object.keys(found).length > 0) return;
    onSubmit(inviteDraftPayload(rows));
  }

  return (
    <Dialog
      open={open}
      title={INVITE_DIALOG_TITLE}
      description={INVITE_DIALOG_DESCRIPTION}
      onOpenChange={onOpenChange}
      nonDismissable={pending}
      className="max-w-2xl"
      actions={
        <>
          <Button variant="secondary" disabled={pending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} onClick={submit}>
            Add members
          </Button>
        </>
      }
    >
      <div data-testid="invite-members-dialog" className="flex flex-col gap-(--space-4)">
        <div className="flex max-h-96 flex-col gap-(--space-3) overflow-y-auto">
          {rows.map((row, index) => (
            <div
              key={row.key}
              data-testid="invite-row"
              className="flex flex-wrap items-start gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-3)"
            >
              <div className="flex min-w-0 flex-col gap-(--space-1)">
                <RowMemberPicker
                  index={index}
                  row={row}
                  candidates={candidates}
                  pending={pending}
                  onPick={(candidate) => updateRows(pickInviteDraftMember(rows, row.key, candidate))}
                />
                {errors[row.key] === undefined ? null : (
                  <span data-testid={`invite-row-error-${index}`} className="text-caption text-(--danger)">
                    {errors[row.key]}
                  </span>
                )}
              </div>
              <RowRolePicker
                index={index}
                row={row}
                candidates={candidates}
                pending={pending}
                onPick={(role) => updateRows(setInviteDraftRole(rows, row.key, role))}
              />
              <IconButton
                label={`Remove row ${index + 1}`}
                disabled={pending || rows.length === 1}
                onClick={() => updateRows(removeInviteDraftRow(rows, row.key))}
              >
                <Trash2 size={14} strokeWidth={1.5} />
              </IconButton>
            </div>
          ))}
        </div>
        <div>
          <Button
            variant="ghost"
            size="small"
            disabled={pending}
            onClick={() => updateRows(appendInviteDraftRow(rows))}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            Add more
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
