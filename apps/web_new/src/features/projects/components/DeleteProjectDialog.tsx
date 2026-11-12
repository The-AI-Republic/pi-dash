// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete-project dialog (SHELL-039; written from
// parity/specs/shell.md §3). Deletion is double-gated: the exact
// project name plus the ritual phrase arm the destructive button,
// and anything less keeps it disabled. Correct entries remove the
// project with a success notice; a refused write toasts and keeps
// the dialog open. Reached from the project list and the settings
// danger zone.
import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { useDeleteProject } from "../api/mutations.js";

/** Confirmation phrase that arms the destructive button. */
export const DELETE_CONFIRM_PHRASE = "delete my project";

export interface DeleteProjectDialogProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Where the view goes after the delete lands; omitted by hosts that stay put. */
  onNavigate?: ((href: string) => void) | undefined;
}

/** True once both confirmations match exactly. Pure for tests. */
export function canDeleteProject(typedName: string, confirm: string, projectName: string): boolean {
  return typedName === projectName && confirm === DELETE_CONFIRM_PHRASE;
}

export function DeleteProjectDialog({
  workspaceSlug,
  projectId,
  projectName,
  open,
  onOpenChange,
  onNavigate,
}: DeleteProjectDialogProps): React.ReactElement {
  const [typedName, setTypedName] = React.useState("");
  const [confirm, setConfirm] = React.useState("");

  // Fresh confirmations per visit: reopening never inherits stale input.
  React.useEffect(() => {
    setTypedName("");
    setConfirm("");
  }, [open]);

  const remove = useDeleteProject();
  const armed = canDeleteProject(typedName, confirm, projectName);

  function submit(): void {
    if (!armed) return;
    remove.mutate(
      { workspaceSlug, projectId, projectName },
      {
        onSuccess: () => {
          onOpenChange(false);
          onNavigate?.(`/${workspaceSlug}/projects`);
        },
      }
    );
  }

  return (
    <Dialog
      title="Delete project"
      description={`Delete ${projectName}? Everything inside it is removed for good and this cannot be undone.`}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="danger" disabled={!armed} loading={remove.isPending} onClick={submit}>
            {remove.isPending ? "Deleting…" : "Delete project"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label={`Enter the project name ${projectName} to continue:`}
          value={typedName}
          onChange={(event) => setTypedName(event.target.value)}
          placeholder="Project name"
          autoComplete="off"
        />
        <Input
          label={`To confirm, type ${DELETE_CONFIRM_PHRASE} below:`}
          value={confirm}
          onChange={(event) => setConfirm(event.target.value)}
          placeholder={`Enter '${DELETE_CONFIRM_PHRASE}'`}
          autoComplete="off"
        />
      </div>
    </Dialog>
  );
}
