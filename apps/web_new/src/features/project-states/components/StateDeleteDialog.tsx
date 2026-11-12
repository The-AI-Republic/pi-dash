// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation for a workflow state (parity/specs/
// project-settings-states.md, PSET-032). The dialog names the state and
// warns the removal is permanent. There is no reassignment step: a state
// that still holds work items is refused by the server with guidance to
// move them first, and the row survives.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface StateDeleteDialogProps {
  /** State awaiting confirmation; null keeps the dialog closed. */
  stateName: string | null;
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function StateDeleteDialog({
  stateName,
  pending,
  onCancel,
  onConfirm,
}: StateDeleteDialogProps): React.ReactElement {
  return (
    <Dialog
      title="Delete this state?"
      description="This removes the state permanently. Work items in it must be moved first."
      open={stateName !== null}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
      actions={
        <>
          <Button variant="ghost" onClick={onCancel} disabled={pending}>
            Cancel
          </Button>
          <Button variant="danger" onClick={onConfirm} disabled={pending}>
            Delete state
          </Button>
        </>
      }
    >
      {stateName === null ? null : (
        <p data-testid="state-delete-message" className="text-body text-(--text)">
          <strong className="font-medium">{stateName}</strong> will be removed permanently. This cannot be undone.
        </p>
      )}
    </Dialog>
  );
}
