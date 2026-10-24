// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Consequence confirmation for deleting a reusable scheduler (AGT-004).

import type { Scheduler } from "@pidash/api-client";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useDeleteScheduler } from "./api.js";

export interface SchedulerDeleteDialogProps {
  workspaceSlug: string;
  scheduler: Scheduler | null;
  onClose: () => void;
}

export function SchedulerDeleteDialog({
  workspaceSlug,
  scheduler,
  onClose,
}: SchedulerDeleteDialogProps): React.ReactElement {
  const remove = useDeleteScheduler(workspaceSlug);
  return (
    <Dialog
      open={scheduler !== null}
      onOpenChange={(next) => {
        if (!next && !remove.isPending) onClose();
      }}
      nonDismissable={remove.isPending}
      title="Delete scheduler"
      description={
        scheduler === null
          ? ""
          : `Deleting ${scheduler.name} stops its project installs from firing. The handle ${scheduler.slug} becomes available for re-creation.`
      }
      actions={
        scheduler === null ? null : (
          <>
            <Button variant="secondary" disabled={remove.isPending} onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="danger"
              loading={remove.isPending}
              onClick={() => remove.mutate(scheduler.id, { onSuccess: onClose })}
            >
              {remove.isPending ? "Deleting…" : "Delete scheduler"}
            </Button>
          </>
        )
      }
    >
      {null}
    </Dialog>
  );
}
