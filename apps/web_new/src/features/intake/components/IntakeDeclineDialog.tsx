// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Decline confirmation (parity/specs/intake.md INT-021). Confirming
// marks the request declined and the screen advances; cancelling
// changes nothing. The caller owns the write and closes on settle.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

/** Decline dialog heading. */
export const DECLINE_DIALOG_TITLE = "Decline work item";

export interface IntakeDeclineDialogProps {
  open: boolean;
  /** The decline write is in flight; controls lock. */
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function IntakeDeclineDialog({
  open,
  pending,
  onCancel,
  onConfirm,
}: IntakeDeclineDialogProps): React.ReactElement {
  return (
    <Dialog
      title={DECLINE_DIALOG_TITLE}
      description="The request will leave triage as declined. This cannot be undone."
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
            Decline
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">Declining is final: the request stays visible under Closed.</p>
    </Dialog>
  );
}
