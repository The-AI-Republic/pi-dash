// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod deletion confirmation (NEWFRONT-542, RUN-015). The copy names what
// happens to issues, runners and history before anything is removed. The
// server can still refuse — with runners attached, with runs in flight, or
// because the pod is the project default — and each refusal has its own
// sentence here so the reason reads like an instruction. While the delete is
// in flight the dialog cannot be dismissed.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import type { Pod } from "@pidash/api-client";
import { notifyError } from "../../../shared/shell/index.js";
import { useDeletePod } from "../api/pods.js";
import { POD_ERROR_TITLE, podDeleteErrorMessage } from "../pods.js";

export const DELETE_POD_TITLE = "Delete pod?";
export const DELETE_POD_BODY =
  "Issues that point at this pod become unassigned, while its runners and every past run are preserved. A pod that still has runners or runs in flight cannot be deleted, and neither can the project's default pod.";

export interface DeletePodDialogProps {
  workspaceSlug: string;
  /** The pod awaiting confirmation; null keeps the dialog closed. */
  pod: Pod | null;
  onCancel: () => void;
  /** Fired after the server accepts the delete, before the dialog closes. */
  onDeleted: (podId: string) => void;
}

export function DeletePodDialog({ workspaceSlug, pod, onCancel, onDeleted }: DeletePodDialogProps): React.ReactElement {
  const remove = useDeletePod(workspaceSlug);

  function confirm(target: Pod): void {
    remove.mutate(target.id, {
      onSuccess: () => {
        onDeleted(target.id);
        onCancel();
      },
      onError: (error) => notifyError(POD_ERROR_TITLE, podDeleteErrorMessage(error)),
    });
  }

  return (
    <Dialog
      open={pod !== null}
      onOpenChange={(next) => {
        if (!next && !remove.isPending) onCancel();
      }}
      title={DELETE_POD_TITLE}
      description={DELETE_POD_BODY}
      nonDismissable={remove.isPending}
      bodyClassName="mt-(--space-4)"
      actions={
        pod === null ? null : (
          <>
            <Button variant="ghost" disabled={remove.isPending} onClick={onCancel}>
              Cancel
            </Button>
            <Button variant="danger" loading={remove.isPending} onClick={() => confirm(pod)}>
              Delete
            </Button>
          </>
        )
      }
    >
      <p className="text-body text-(--text)">{pod?.name ?? ""}</p>
    </Dialog>
  );
}
