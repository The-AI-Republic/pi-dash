// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Delete confirmation (parity/specs/issues.md §11, ISS-135). The alert
// names the record by its short key and warns the removal is permanent.
// Submitting first checks creator-or-admin standing locally; a viewer
// without it gets the permission notice and no request. The server's own
// 403 maps to the same notice, and a landed delete toasts the noun.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useDeleteWorkItem, type WorkItemScope } from "./api.js";
import {
  DELETE_FAILED_BODY,
  DELETE_FAILED_TITLE,
  DELETE_PERMISSION_TITLE,
  DELETE_SUCCESS_TITLE,
  deleteBody,
  deleteHeading,
  deleteSuccessBody,
  failureDetail,
  isDeletePermissionError,
  kindNoun,
  mayDeleteWorkItem,
  pendingLabel,
  workItemKey,
  type WorkItemKind,
} from "./rules.js";

export interface DeleteWorkItemDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  scope: WorkItemScope;
  projectIdentifier: string;
  sequenceId: number;
  kind?: WorkItemKind | undefined;
  /** The signed-in user's id; null while the session read is pending. */
  viewerId: string | null | undefined;
  /** The record creator's id as the server sent it. */
  creatorId: string | null | undefined;
  /** The viewer's role on the owning project. */
  projectRole: number | null | undefined;
  /** After a landed delete (the caller navigates away from a detail page). */
  onDeleted?: (() => void) | undefined;
}

export function DeleteWorkItemDialog({
  open,
  onOpenChange,
  scope,
  projectIdentifier,
  sequenceId,
  kind = "issue",
  viewerId,
  creatorId,
  projectRole,
  onDeleted,
}: DeleteWorkItemDialogProps): React.ReactElement {
  const remove = useDeleteWorkItem(scope);
  const key = workItemKey(projectIdentifier, sequenceId);

  function confirm(): void {
    if (!mayDeleteWorkItem({ viewerId, creatorId, projectRole })) {
      notifyError(DELETE_PERMISSION_TITLE);
      onOpenChange(false);
      return;
    }
    remove.mutate(undefined, {
      onSuccess: () => {
        notifySuccess(DELETE_SUCCESS_TITLE, deleteSuccessBody(kind));
        onOpenChange(false);
        onDeleted?.();
      },
      onError: (error) => {
        if (isDeletePermissionError(error)) {
          notifyError(DELETE_PERMISSION_TITLE);
        } else {
          notifyError(DELETE_FAILED_TITLE, failureDetail(error, DELETE_FAILED_BODY));
        }
      },
    });
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => onOpenChange(next)}
      title={deleteHeading(kind)}
      description={deleteBody(kind, key)}
      className="border-(--danger)"
      actions={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="danger" loading={remove.isPending} onClick={confirm}>
            {pendingLabel("delete", remove.isPending)}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">
        Only the person who created the {kindNoun(kind)} or a project admin can delete it.
      </p>
    </Dialog>
  );
}
