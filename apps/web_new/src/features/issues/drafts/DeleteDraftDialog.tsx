// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation for a workspace draft (parity/specs/issues.md §12,
// ISS-140). The dialog only asks; the caller owns the standing check, the
// write and the failure toast, because a refusal reads differently from a
// generic failure and the caller already holds the server answer.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export const DELETE_DRAFT_TITLE = "Delete draft";
export const DELETE_DRAFT_BODY = "Are you sure you want to delete this draft? This can't be undone.";

export interface DeleteDraftDialogProps {
  open: boolean;
  /** Name of the draft being deleted, for the heading's context. */
  draftName: string;
  /** True while the delete write is in flight. */
  deleting?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function DeleteDraftDialog({
  open,
  draftName,
  deleting = false,
  onCancel,
  onConfirm,
}: DeleteDraftDialogProps): React.ReactElement {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      title={DELETE_DRAFT_TITLE}
      description={DELETE_DRAFT_BODY}
      actions={
        <>
          <Button variant="secondary" onClick={onCancel} disabled={deleting}>
            Cancel
          </Button>
          <Button variant="danger" onClick={onConfirm} disabled={deleting}>
            {deleting ? "Deleting…" : "Delete"}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text)">{draftName}</p>
    </Dialog>
  );
}
