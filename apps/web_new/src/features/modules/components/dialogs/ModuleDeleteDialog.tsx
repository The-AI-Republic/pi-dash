// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module delete confirmation (parity/specs/modules.md §7.5, MOD-025). The
// body names the module and states that its data is gone for good and
// that the action cannot be undone. Confirming deletes and shows the
// success notice; the caller decides where the user lands afterwards (the
// list for a row/card delete, the modules list for the detail page and
// the peek). A 403 is the server's narrower rule — only an admin or the
// creator may delete — so it gets its own notice and leaves the module
// untouched; every other failure reports the server's sentence.

import { useQueryClient } from "@tanstack/react-query";
import { Button, Dialog } from "@pidash/kit";
import { deleteModule, type ModuleRow } from "@pidash/api-client";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { notifyError, notifySuccess } from "../../../../shared/shell/index.js";
import { invalidateAfterModuleWrite } from "../../api/invalidate.js";
import {
  isModuleDeletePermissionError,
  MODULE_DELETE_HEADING,
  MODULE_DELETE_PERMISSION_TITLE,
  MODULE_FAILURE_TITLE,
  MODULE_SUCCESS_TITLE,
  moduleDeleteBody,
  moduleDeleteNotice,
  moduleFailureBody,
} from "../../lib/moduleForm.js";

export interface ModuleDeleteDialogProps {
  /** The module awaiting confirmation; null keeps the dialog closed. */
  module: Pick<ModuleRow, "id" | "name"> | null;
  workspaceSlug: string;
  projectId: string;
  onOpenChange: (open: boolean) => void;
  /** Runs after the delete lands, before the dialog closes. */
  onDeleted?: ((module: Pick<ModuleRow, "id" | "name">) => void) | undefined;
}

export function ModuleDeleteDialog({
  module,
  workspaceSlug,
  projectId,
  onOpenChange,
  onDeleted,
}: ModuleDeleteDialogProps): React.ReactElement | null {
  const queryClient = useQueryClient();
  const [submitting, setSubmitting] = React.useState(false);

  async function confirm(target: Pick<ModuleRow, "id" | "name">): Promise<void> {
    setSubmitting(true);
    try {
      await deleteModule(getAppClient(), workspaceSlug, projectId, target.id);
      await invalidateAfterModuleWrite(queryClient, workspaceSlug, projectId, target.id);
      notifySuccess(MODULE_SUCCESS_TITLE, moduleDeleteNotice(target.name));
      onOpenChange(false);
      onDeleted?.(target);
    } catch (error) {
      if (isModuleDeletePermissionError(error)) {
        // The module stays; the refusal speaks for itself.
        notifyError(MODULE_DELETE_PERMISSION_TITLE, moduleFailureBody(error));
      } else {
        notifyError(MODULE_FAILURE_TITLE, moduleFailureBody(error));
      }
      onOpenChange(false);
    } finally {
      setSubmitting(false);
    }
  }

  return module === null ? null : (
    <Dialog
      title={MODULE_DELETE_HEADING}
      open
      onOpenChange={() => {
        onOpenChange(false);
      }}
      closeButton={false}
    >
      <p data-testid="module-delete-body" className="text-body text-(--text)">
        {moduleDeleteBody(module.name)}
      </p>
      <div className="mt-(--space-6) flex justify-end gap-(--space-3)">
        <Button
          variant="secondary"
          type="button"
          disabled={submitting}
          onClick={() => {
            onOpenChange(false);
          }}
        >
          Cancel
        </Button>
        <Button
          variant="danger"
          type="button"
          loading={submitting}
          disabled={submitting}
          onClick={() => {
            void confirm(module);
          }}
        >
          Delete
        </Button>
      </div>
    </Dialog>
  );
}
