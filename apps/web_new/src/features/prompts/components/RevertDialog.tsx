// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Revert confirmation (parity/specs/agents.md §6, AGT-027). The copy is
// scope-specific and warns the revert cannot be undone; confirming fires
// the revert while the dialog holds, cancelling keeps the override.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import type { PromptEditorScope } from "../lib/sections.js";

/** Scope-specific revert copy; the oracle matches its key phrases. */
export function revertDialogBody(scope: PromptEditorScope): string {
  if (scope === "workspace") {
    return "The workspace default will be discarded and this can't be undone. Afterwards every member falls back to the registry default.";
  }
  return "Your override will be discarded and this can't be undone. Runs you trigger will use the workspace default again.";
}

export interface RevertDialogProps {
  scope: PromptEditorScope;
  open: boolean;
  /** The revert is in flight: the dialog holds until it settles. */
  pending: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function RevertDialog({ scope, open, pending, onConfirm, onCancel }: RevertDialogProps): React.ReactElement {
  return (
    <Dialog
      title="Revert to default?"
      description={revertDialogBody(scope)}
      open={open}
      onOpenChange={onCancel}
      nonDismissable={pending}
      actions={
        <>
          <Button variant="secondary" onClick={onCancel} disabled={pending}>
            Cancel
          </Button>
          <Button variant="danger" loading={pending} onClick={onConfirm}>
            {pending ? "Reverting…" : "Revert"}
          </Button>
        </>
      }
    >
      {null}
    </Dialog>
  );
}
