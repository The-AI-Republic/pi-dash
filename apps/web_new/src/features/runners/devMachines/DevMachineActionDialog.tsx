// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Machine action confirmation (NEWFRONT-555, RUN-038–040). One dialog serves
// rotate, revoke and delete. While the write is in flight every dismissal
// path is inert and the confirm shows its working state; a failure keeps the
// dialog open for a retry while the page toasts the reason.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { MACHINE_ACTION_COPY, type MachineAction } from "./model.js";

export interface DevMachineActionDialogProps {
  open: boolean;
  action: MachineAction;
  machineName: string;
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function DevMachineActionDialog({
  open,
  action,
  machineName,
  pending,
  onCancel,
  onConfirm,
}: DevMachineActionDialogProps): React.ReactElement {
  const copy = MACHINE_ACTION_COPY[action];
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !pending) onCancel();
      }}
      title={copy.title}
      description={copy.body}
      nonDismissable={pending}
      bodyClassName="mt-(--space-4)"
      actions={
        <>
          <Button variant="ghost" aria-disabled={pending || undefined} onClick={() => !pending && onCancel()}>
            Cancel
          </Button>
          <Button variant="danger" loading={pending} onClick={onConfirm}>
            {copy.confirm}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text)">
        <span className="font-medium">{machineName}</span>
      </p>
    </Dialog>
  );
}
