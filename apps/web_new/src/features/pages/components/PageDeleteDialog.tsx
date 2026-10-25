// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation for an already-shelved page
// (parity/specs/pages.md §3.8, PAGE-016). The host owns the open state and
// the write: confirming calls back while the button holds its busy state,
// cancelling only closes. The body names the page and states the deletion
// is permanent and cannot be undone; the oracle reads both literally.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { pageDisplayTitle } from "../lib/pageList.js";

export interface PageDeleteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Name of the page about to be removed; shown in the warning. */
  pageName: string;
  /** The delete request is in flight; the confirm control holds it. */
  pending: boolean;
  onConfirm: () => void;
}

export function PageDeleteDialog({
  open,
  onOpenChange,
  pageName,
  pending,
  onConfirm,
}: PageDeleteDialogProps): React.ReactElement {
  const title = pageDisplayTitle(pageName);
  return (
    <Dialog
      title="Delete page"
      open={open}
      onOpenChange={(next) => {
        if (!next && pending) return;
        onOpenChange(next);
      }}
      actions={
        <>
          <Button variant="secondary" type="button" disabled={pending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="danger" type="button" loading={pending} disabled={pending} onClick={onConfirm}>
            {pending ? "Deleting…" : "Delete"}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text)">
        Are you sure you want to delete <strong className="font-medium break-words">{title}</strong>? The page will be
        deleted permanently. This action cannot be undone.
      </p>
    </Dialog>
  );
}
