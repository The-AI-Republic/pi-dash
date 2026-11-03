// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Shared remove confirmation (parity/specs/workspace-settings.md §13,
// WSET-010/013). One dialog withdraws an invitation, removes another
// member or leaves the workspace; the title names the address, the person
// or the actor. It closes after a confirm whether or not the write landed
// — the caller's notice carries the outcome — and Escape or Cancel closes
// it untouched. While a write is in flight the dialog stays put. The page
// mounts it only with a target, so every dismissal has something to drop.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { removeDialogCopy, type RemoveTarget } from "./memberRules.js";

export interface RemoveTargetDialogProps {
  target: RemoveTarget;
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function RemoveTargetDialog({
  target,
  pending,
  onCancel,
  onConfirm,
}: RemoveTargetDialogProps): React.ReactElement {
  const copy = removeDialogCopy(target);
  return (
    <Dialog
      open
      title={copy.title}
      description={copy.body}
      onOpenChange={onCancel}
      nonDismissable={pending}
      actions={
        <>
          <Button variant="secondary" disabled={pending} onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="danger" loading={pending} onClick={onConfirm}>
            {copy.confirm}
          </Button>
        </>
      }
    >
      <div />
    </Dialog>
  );
}
