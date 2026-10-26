// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archive confirmation (parity/specs/modules.md §7.4, MOD-026). The body
// names the module and says the archive can be restored later; cancelling
// writes nothing. Confirming posts the archive, refreshes the module
// reads, shows the success notice and lands on the modules list. A failed
// write shows the error notice and leaves the module where it was: the
// dialog closes so the viewer can retry, and nothing navigates, because
// the module is still live (the old store reported success here, bug
// NEWFRONT-233). The caller owns the landing address so the archived
// host can stay put if it ever opens this dialog.

import { useQueryClient } from "@tanstack/react-query";
import { Button, Dialog } from "@pidash/kit";
import { archiveModule, type ModuleRow } from "@pidash/api-client";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { invalidateAfterModuleWrite } from "../../api/invalidate.js";
import {
  MODULE_ARCHIVE_CONFIRM,
  MODULE_ARCHIVE_SUCCESS_BODY,
  MODULE_ARCHIVE_SUCCESS_TITLE,
  moduleArchiveDialogBody,
  moduleArchiveDialogTitle,
  moduleArchiveLanding,
} from "../../lib/moduleArchive.js";
import { MODULE_FAILURE_TITLE, moduleFailureBody } from "../../lib/moduleForm.js";

/** The module the confirmation names and archives. */
export type ModuleArchiveTarget = Pick<ModuleRow, "id" | "name">;

export interface ModuleArchiveDialogProps {
  /** The module awaiting confirmation; null keeps the dialog closed. */
  module: ModuleArchiveTarget | null;
  workspaceSlug: string;
  projectId: string;
  /** Dismiss the confirmation: cancel, the backdrop, Escape or a settled write. */
  onClose: () => void;
  /** Where a landed archive lands the viewer; null stays put. */
  onArchived?: ((module: ModuleArchiveTarget) => void) | undefined;
}

export function ModuleArchiveDialog({
  module,
  workspaceSlug,
  projectId,
  onClose,
  onArchived,
}: ModuleArchiveDialogProps): React.ReactElement | null {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [submitting, setSubmitting] = React.useState(false);

  async function confirm(target: ModuleArchiveTarget): Promise<void> {
    setSubmitting(true);
    try {
      await archiveModule(getAppClient(), workspaceSlug, projectId, target.id);
      await invalidateAfterModuleWrite(queryClient, workspaceSlug, projectId, target.id);
      notifySuccess(MODULE_ARCHIVE_SUCCESS_TITLE, MODULE_ARCHIVE_SUCCESS_BODY);
      onClose();
      if (onArchived === undefined) {
        const landing = moduleArchiveLanding(workspaceSlug, projectId);
        void navigate({ to: landing.to, search: landing.search, replace: landing.replace });
      } else {
        onArchived(target);
      }
    } catch (error) {
      // The module stayed live: say so and navigate nowhere.
      notifyError(MODULE_FAILURE_TITLE, moduleFailureBody(error));
      onClose();
    } finally {
      setSubmitting(false);
    }
  }

  return module === null ? null : (
    <Dialog
      title={moduleArchiveDialogTitle(module.name)}
      open
      // The confirmation is always open while a module is targeted, so
      // every change it reports is a dismissal (Escape, backdrop, Cancel).
      onOpenChange={() => {
        onClose();
      }}
      closeButton={false}
    >
      <p data-testid="module-archive-body" className="text-body text-(--text)">
        {moduleArchiveDialogBody(module.name)}
      </p>
      <div className="mt-(--space-6) flex justify-end gap-(--space-3)">
        <Button variant="secondary" type="button" disabled={submitting} onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          type="button"
          loading={submitting}
          disabled={submitting}
          onClick={() => {
            void confirm(module);
          }}
        >
          {MODULE_ARCHIVE_CONFIRM}
        </Button>
      </div>
    </Dialog>
  );
}
