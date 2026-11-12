// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Completed-cycle prompt above the issues list (parity/specs/issues.md
// §18, ISS-224). A finished cycle is closed to edits; while it still
// holds work that could move, the row also offers handing that work to
// a cycle that is still running.

import { Button } from "@pidash/kit";
import { CircleAlert } from "lucide-react";
import * as React from "react";

/** The note a finished cycle shows whether or not a move is offered. */
export const COMPLETED_CYCLE_NOTE = "This cycle has ended, so its work items can no longer be edited here.";

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
          Move unfinished work
        </Button>
      ) : null}
    </div>
  );
}
