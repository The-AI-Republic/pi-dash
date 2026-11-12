// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation for a custom workspace view (parity/specs/views.md
// §10, VIEW-036). Confirming DELETEs the record, drops it from the list
// cache and toasts; a failed delete toasts the error and keeps both the
// row and the dialog. Hosts render this conditionally with the record
// under delete; the detail host also navigates away through `onDeleted`.

import { useQueryClient } from "@tanstack/react-query";
import { deleteWorkspaceView, type SavedView, type SavedViewList } from "@pidash/api-client";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { viewKeys } from "../api/keys.js";
import { failureDetail } from "./failureDetail.js";

export interface DeleteWorkspaceViewDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspaceSlug: string;
  view: SavedView;
  /** Fired after a successful delete, before the dialog closes. */
  onDeleted?: ((viewId: string) => void) | undefined;
}

const DELETE_FAILURE_BODY = "The view was kept — check the details and try again.";

export function DeleteWorkspaceViewDialog({
  open,
  onOpenChange,
  workspaceSlug,
  view,
  onDeleted,
}: DeleteWorkspaceViewDialogProps): React.ReactElement {
  const queryClient = useQueryClient();
  const [deleting, setDeleting] = React.useState(false);

  async function confirm(): Promise<void> {
    setDeleting(true);
    try {
      await deleteWorkspaceView(getAppClient(), workspaceSlug, view.id);
      queryClient.setQueryData<SavedViewList>(viewKeys.workspaceList(workspaceSlug), (current) =>
        current === undefined ? current : current.filter((row) => row.id !== view.id)
      );
      await queryClient.invalidateQueries({ queryKey: viewKeys.workspaceList(workspaceSlug) });
      notifySuccess("View deleted successfully");
      onDeleted?.(view.id);
      onOpenChange(false);
    } catch (error) {
      notifyError("Failed to delete view", failureDetail(error, DELETE_FAILURE_BODY));
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Dialog
      title="Are you sure you want to delete this view?"
      description="The view's layout, sort, filter and display choices are permanently deleted with it. There is no way to restore them."
      open={open}
      onOpenChange={(next, details) => {
        // While the delete flies an outside press is swallowed and every
        // other dismissal (Escape, the footer) still closes.
        if (deleting && details.reason === "outside-press") return;
        onOpenChange(next);
      }}
      closeButton={false}
    >
      <div className="mt-(--space-6) flex justify-end gap-(--space-3)">
        <Button
          variant="secondary"
          type="button"
          disabled={deleting}
          onClick={() => {
            onOpenChange(false);
          }}
        >
          Cancel
        </Button>
        <Button
          variant="danger"
          type="button"
          disabled={deleting}
          onClick={() => {
            void confirm();
          }}
        >
          {deleting ? "Deleting…" : "Delete"}
        </Button>
      </div>
    </Dialog>
  );
}
