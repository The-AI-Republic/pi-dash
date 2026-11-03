// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive confirmation (parity/specs/issues.md §11, ISS-134). A centered
// dialog names the record by its short key and says archives can be
// restored later; confirming posts the archive stamp, toasts and closes.
// Cancel and Escape close without a request.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { useArchiveWorkItem, type WorkItemScope } from "./api.js";
import {
  ARCHIVE_FAILED_BODY,
  ARCHIVE_FAILED_TITLE,
  ARCHIVE_SUCCESS_BODY,
  ARCHIVE_SUCCESS_TITLE,
  archiveBody,
  archiveHeading,
  failureDetail,
  kindNoun,
  pendingLabel,
  type WorkItemKind,
} from "./rules.js";

export interface ArchiveWorkItemDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  scope: WorkItemScope;
  /** Short code of the owning project, e.g. "WEB". */
  projectIdentifier: string;
  sequenceId: number;
  kind?: WorkItemKind | undefined;
  /** After a landed archive (the row list usually refetches by itself). */
  onArchived?: (() => void) | undefined;
}

export function ArchiveWorkItemDialog({
  open,
  onOpenChange,
  scope,
  projectIdentifier,
  sequenceId,
  kind = "issue",
  onArchived,
}: ArchiveWorkItemDialogProps): React.ReactElement {
  const archive = useArchiveWorkItem(scope);
  const noun = kindNoun(kind);

  function confirm(): void {
    archive.mutate(undefined, {
      onSuccess: () => {
        notifySuccess(ARCHIVE_SUCCESS_TITLE, ARCHIVE_SUCCESS_BODY);
        onOpenChange(false);
        onArchived?.();
      },
      onError: (error) => {
        notifyError(ARCHIVE_FAILED_TITLE, failureDetail(error, ARCHIVE_FAILED_BODY));
      },
    });
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => onOpenChange(next)}
      title={archiveHeading(kind, projectIdentifier, sequenceId)}
      description={archiveBody(kind)}
      actions={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" loading={archive.isPending} onClick={confirm}>
            {pendingLabel("archive", archive.isPending)}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text)">
        The {noun} moves out of the project's active lists. Its identifier, {projectIdentifier} {sequenceId}, stays
        reserved for a restore.
      </p>
    </Dialog>
  );
}
