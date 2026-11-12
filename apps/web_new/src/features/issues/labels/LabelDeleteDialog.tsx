// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete-label dialog (parity/specs/issues.md §19, ISS-228): names the
// label and warns it leaves every carrying work item and filtering
// view with no undo. Confirming deletes; a refused delete notices
// through the mutation and keeps the dialog open.

import type { Label } from "@pidash/api-client";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useDeleteLabel } from "./api.js";

export interface LabelDeleteDialogProps {
  workspaceSlug: string;
  projectId: string;
  /** The label under confirmation; null closes the dialog. */
  label: Label | null;
  onClose: () => void;
}

export function LabelDeleteDialog({
  workspaceSlug,
  projectId,
  label,
  onClose,
}: LabelDeleteDialogProps): React.ReactElement {
  const remove = useDeleteLabel(workspaceSlug, projectId);
  return (
    <Dialog
      title="Delete label"
      description={
        label === null
          ? ""
          : `Deleting ${label.name} will remove the label from all the work items and views filtering on it. This cannot be undone.`
      }
      open={label !== null}
      onOpenChange={onClose}
      nonDismissable={remove.isPending}
      actions={
        label === null ? null : (
          <>
            <Button variant="secondary" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="danger"
              loading={remove.isPending}
              onClick={() => remove.mutate(label.id, { onSuccess: onClose })}
            >
              {remove.isPending ? "Deleting…" : "Delete label"}
            </Button>
          </>
        )
      }
    >
      {null}
    </Dialog>
  );
}
