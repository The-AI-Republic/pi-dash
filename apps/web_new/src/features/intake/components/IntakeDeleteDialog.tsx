// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation (parity/specs/intake.md INT-025). Deleting a
// request that was never accepted removes its work item too; deleting
// an accepted request removes only the intake entry and the project
// work item stays. The caller owns the write and closes on settle.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

/** Delete dialog heading while the work item goes with the request. */
export const DELETE_DIALOG_TITLE = "Delete work item";
/** Delete dialog heading once the work item has joined the project. */
export const DELETE_ACCEPTED_DIALOG_TITLE = "Delete request";

export interface IntakeDeleteDialogProps {
  open: boolean;
  /** The delete is in flight; controls lock. */
  pending: boolean;
  /** The request is accepted, so its work item survives the delete. */
  accepted: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function IntakeDeleteDialog({
  open,
  pending,
  accepted,
  onCancel,
  onConfirm,
}: IntakeDeleteDialogProps): React.ReactElement {
  return (
    <Dialog
      title={accepted ? DELETE_ACCEPTED_DIALOG_TITLE : DELETE_DIALOG_TITLE}
      description={
        accepted
          ? "The request will leave intake. Its project work item stays. This cannot be undone."
          : "The request and its work item will be permanently removed. This cannot be undone."
      }
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      actions={
        <>
          <Button variant="secondary" onClick={onCancel} disabled={pending}>
            Cancel
          </Button>
          <Button variant="danger" onClick={onConfirm} disabled={pending} loading={pending}>
            Delete
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">
        {accepted
          ? "Only the intake entry goes; the project keeps working from the work item."
          : "There is no way back: both the intake entry and its work item go."}
      </p>
    </Dialog>
  );
}
