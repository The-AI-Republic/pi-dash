// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item delete confirmation (shell spec, SHELL-090). The palette's
// Delete entry opens this from the mounting route (the palette closes
// first); confirming removes the item, toasts, and sends the caller
// back to the list. Only the creator or a project admin may delete —
// anyone else gets the permission message and stays put.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface DeleteWorkItemDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** "Epic", "Sub-work item" or "Work item". */
  noun: string;
  /** Item title shown for confidence. */
  title: string;
  /** True while the delete write is in flight. */
  deleting: boolean;
  /** Run the delete write. */
  onConfirm: () => void;
}

export function DeleteWorkItemDialog({
  open,
  onOpenChange,
  noun,
  title,
  deleting,
  onConfirm,
}: DeleteWorkItemDialogProps): React.ReactElement {
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`Delete ${noun.toLowerCase()}?`}
      description={`“${title}” will be permanently removed. This cannot be undone.`}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="danger" loading={deleting} onClick={onConfirm}>
            Delete
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">
        Only the creator or a project admin can delete this {noun.toLowerCase()}.
      </p>
    </Dialog>
  );
}
