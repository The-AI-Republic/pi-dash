// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Completed-cycle prompt above the issues list (parity/specs/issues.md
// §18, ISS-224). Completed cycles are not editable; while one still
// holds transferable work and has no frozen progress snapshot, the row
// also offers moving that work to another cycle.

import { Button } from "@pidash/kit";
import { CircleAlert } from "lucide-react";
import * as React from "react";

/** The note completed cycles show whether or not transfer is offered. */
export const COMPLETED_CYCLE_NOTE = "Completed cycles are not editable.";

export interface TransferIssuesPromptProps {
  /** Whether the transfer button renders. */
  canTransfer: boolean;
  disabled?: boolean;
  onOpen: () => void;
}

export function TransferIssuesPrompt({
  canTransfer,
  disabled = false,
  onOpen,
}: TransferIssuesPromptProps): React.ReactElement {
  return (
    <div className="flex flex-wrap items-center justify-between gap-(--space-3) border-b border-(--border) px-(--space-4) py-(--space-3)">
      <p className="text-meta flex items-center gap-(--space-2) text-(--text-muted)">
        <CircleAlert size={14} strokeWidth={1.5} aria-hidden="true" />
        {COMPLETED_CYCLE_NOTE}
      </p>
      {canTransfer ? (
        <Button variant="primary" size="small" onClick={onOpen} disabled={disabled}>
          Transfer work items
        </Button>
      ) : null}
    </div>
  );
}
