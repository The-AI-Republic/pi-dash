// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation for a saved project view (parity/specs/views.md
// §6, VIEW-016). The dialog only asks; the caller owns the write, the
// cache removal, the navigation back to the list and the notices.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export const DELETE_VIEW_TITLE = "Are you sure you want to delete this view?";
export const DELETE_VIEW_BODY =
  "The layout, sort, filter and display choices in this view will be permanently deleted " +
  "without any way to restore them.";

export interface DeleteViewDialogProps {
  open: boolean;
  /** True while the delete write is in flight. */
  deleting?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function DeleteViewDialog({
  open,
  deleting = false,
  onCancel,
  onConfirm,
}: DeleteViewDialogProps): React.ReactElement {
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      title={DELETE_VIEW_TITLE}
      description={DELETE_VIEW_BODY}
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
      <p className="text-body text-(--text)">This cannot be undone.</p>
    </Dialog>
  );
}
