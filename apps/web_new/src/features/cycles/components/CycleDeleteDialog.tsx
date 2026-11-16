// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete-cycle confirmation (parity/specs/cycles.md §8, CYC-022). The
// host owns the open state and the write: confirming calls back while
// the button shows its pending state, and cancelling only closes.
// The heading names the action, the body names the cycle and states
// that deletion is permanent; both are parity reads.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface CycleDeleteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The cycle about to be removed; named in the body. */
  cycleName: string;
  /** The write is in flight; the confirm button holds it. */
  pending: boolean;
  onConfirm: () => void;
}

export function CycleDeleteDialog({
  open,
  onOpenChange,
  cycleName,
  pending,
  onConfirm,
}: CycleDeleteDialogProps): React.ReactElement {
  return (
    <Dialog
      title="Delete cycle"
      description={`"${cycleName}" will be permanently deleted. This can't be undone.`}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" type="button" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="danger" type="button" loading={pending} disabled={pending} onClick={onConfirm}>
            {pending ? "Deleting…" : "Delete"}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">The cycle leaves every list and group it renders in.</p>
    </Dialog>
  );
}
