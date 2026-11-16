// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner row-action confirmation (NEWFRONT-540, RUN-010). One dialog serves
// delete, revoke and a built-in engine's "Remove from project": a heading, the consequence copy, and a confirm button
// sharing the action's name. While the write is in flight the confirm shows
// its working state, every dismissal path is inert, and a failure leaves the
// dialog open for a retry while the caller toasts the reason.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface RunnerConfirmDialogProps {
  open: boolean;
  action: RunnerConfirmAction;
  runnerName: string;
  /** A built-in engine row: the copy says its folder is kept. */
  engine?: boolean;
  pending: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** The row actions this dialog confirms. */
export type RunnerConfirmAction = "delete" | "revoke" | "remove";

export const DELETE_RUNNER_TITLE = "Delete runner?";
export const DELETE_RUNNER_BODY =
  "The runner row is removed and the daemon is forced offline. Historic runs are preserved, with their runner reference cleared. Deleting a runner does not uninstall the agent CLI (such as Codex or Claude) on the machine.";
export const REVOKE_RUNNER_TITLE = "Revoke runner?";
export const REVOKE_RUNNER_BODY =
  "The runner's credentials are invalidated and any in-flight runs are cancelled, but the row stays in the list. To attach the machine again, delete this row and add a new runner from the target machine.";

export const REMOVE_ENGINE_TITLE = "Remove from project?";
export const REMOVE_ENGINE_BODY =
  "The built-in engine stops taking this project's work and its runner is revoked; runs queued for it go back to the pod for another runner. The engine's folder on the computer is kept, so turning the engine on for this project again picks up where it left off.";
/** Label of a built-in engine's remove action (design §11). */
export const REMOVE_ENGINE_LABEL = "Remove from project";
export const DELETE_ENGINE_BODY =
  "The removed engine's row is cleared from the list. Historic runs are preserved, with their runner reference cleared. The engine's folder on the computer is kept.";

export function runnerConfirmCopy(
  action: RunnerConfirmAction,
  engine = false
): { title: string; body: string; confirm: string } {
  if (action === "remove")
    return { title: REMOVE_ENGINE_TITLE, body: REMOVE_ENGINE_BODY, confirm: REMOVE_ENGINE_LABEL };
  if (action === "delete") {
    return { title: DELETE_RUNNER_TITLE, body: engine ? DELETE_ENGINE_BODY : DELETE_RUNNER_BODY, confirm: "Delete" };
  }
  return { title: REVOKE_RUNNER_TITLE, body: REVOKE_RUNNER_BODY, confirm: "Revoke" };
}

export function RunnerConfirmDialog({
  open,
  action,
  runnerName,
  engine = false,
  pending,
  onCancel,
  onConfirm,
}: RunnerConfirmDialogProps): React.ReactElement {
  const copy = runnerConfirmCopy(action, engine);
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
