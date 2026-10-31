// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Discard guard for the create dialog (parity/specs/issues.md §11,
// ISS-120). Closing a new item that already holds work asks whether to
// keep those values as a workspace draft, throw them away, or go back to
// the form. The dialog never decides on its own: the caller sanitizes
// the form and owns the draft write, so a clean form closes silently.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface DiscardDraftDialogProps {
  open: boolean;
  /** Cancel path: the caller returns to the form. */
  onCancel: () => void;
  /** Throw the form away. */
  onDiscard: () => void;
  /** Keep the form as a draft; resolves once the draft write settles. */
  onSaveDraft: () => Promise<void> | void;
  saving?: boolean;
  /** Failure note under the body when the draft write did not land. */
  error?: string | null;
}

export function DiscardDraftDialog({
  open,
  onCancel,
  onDiscard,
  onSaveDraft,
  saving = false,
  error = null,
}: DiscardDraftDialogProps): React.ReactElement {
  return (
    <Dialog
      title="Save this draft?"
      description="You can finish it later from your drafts."
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      actions={
        <>
          <Button variant="ghost" onClick={onDiscard} disabled={saving}>
            Discard
          </Button>
          <Button variant="secondary" onClick={onCancel} disabled={saving}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => {
              void onSaveDraft();
            }}
            disabled={saving}
          >
            {saving ? "Saving…" : "Save to Drafts"}
          </Button>
        </>
      }
    >
      <p className="text-body text-(--text-muted)">
        Saving keeps the title, description and properties you entered without filing a work item yet.
      </p>
      {error === null ? null : (
        <p role="alert" className="text-meta mt-(--space-3) text-(--danger)">
          {error}
        </p>
      )}
    </Dialog>
  );
}
