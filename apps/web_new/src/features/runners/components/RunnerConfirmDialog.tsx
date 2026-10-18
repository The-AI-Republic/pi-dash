// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner row-action confirmation (NEWFRONT-540, RUN-010). One dialog serves
// delete and revoke: a heading, the consequence copy, and a confirm button
// sharing the action's name. While the write is in flight the confirm shows
// its working state, every dismissal path is inert, and a failure leaves the
// dialog open for a retry while the caller toasts the reason.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface RunnerConfirmDialogProps {
  open: boolean;
  action: "delete" | "revoke";
  runnerName: string;
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export const DELETE_RUNNER_TITLE = "Delete runner?";
export const DELETE_RUNNER_BODY =
  "The runner row is removed and the daemon is forced offline. Historic runs are preserved, with their runner reference cleared. Deleting a runner does not uninstall the agent CLI (such as Codex or Claude) on the machine.";
export const REVOKE_RUNNER_TITLE = "Revoke runner?";
export const REVOKE_RUNNER_BODY =
  "The runner's credentials are invalidated and any in-flight runs are cancelled, but the row stays in the list. To attach the machine again, delete this row and add a new runner from the target machine.";

export function runnerConfirmCopy(action: "delete" | "revoke"): { title: string; body: string; confirm: string } {
  return action === "delete"
    ? { title: DELETE_RUNNER_TITLE, body: DELETE_RUNNER_BODY, confirm: "Delete" }
    : { title: REVOKE_RUNNER_TITLE, body: REVOKE_RUNNER_BODY, confirm: "Revoke" };
}

export function RunnerConfirmDialog({
  open,
  action,
  runnerName,
  pending,
  onCancel,
  onConfirm,
}: RunnerConfirmDialogProps): React.ReactElement {
  const copy = runnerConfirmCopy(action);
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
        <span className="font-medium">{runnerName}</span>
      </p>
    </Dialog>
  );
}
