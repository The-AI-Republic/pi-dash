// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive-cycle confirmation (parity/specs/cycles.md §9, CYC-024).
// The host owns the open state and the write: confirming calls back
// while the button shows its pending state, and cancelling only
// closes. The heading names the cycle and the body notes it can be
// restored later; both are parity reads.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface CycleArchiveDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The cycle about to be archived; named in the heading. */
  cycleName: string;
  /** The write is in flight; the confirm button holds it. */
  pending: boolean;
  onConfirm: () => void;
}

export function CycleArchiveDialog({
  open,
  onOpenChange,
  cycleName,
  pending,
  onConfirm,
}: CycleArchiveDialogProps): React.ReactElement {
  return (
    <Dialog
      title={`Archive cycle ${cycleName}`}
      description={`"${cycleName}" will move to project archives. You can restore it at any time.`}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" type="button" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" type="button" loading={pending} disabled={pending} onClick={onConfirm}>
            {pending ? "Archiving…" : "Archive"}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">The cycle leaves the live list and groups.</p>
    </Dialog>
  );
}
