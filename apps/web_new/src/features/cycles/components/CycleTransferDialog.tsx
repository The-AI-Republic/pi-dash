// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer dialog (parity/specs/cycles.md §12, CYC-031). The one dialog
// behind both entry points: a searchable picker of the project's open
// cycles — never the source, never finished or archived. Selecting a
// destination moves the remainder at once; success closes, a refusal
// reports and leaves the dialog's state intact for another pick.

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { usePickerCycles } from "../../../shared/pickers/reference.js";
import { useTransferCycleWork } from "../api/cycleTransfer.js";
import { transferStatusPill, transferTargetOptions } from "../lib/cycleTransfer.js";

/** Dialog heading; the banner action carries the same words. */
export const CYCLE_TRANSFER_TITLE = "Transfer work items";
/** Search hint the parity driver waits on. */
export const CYCLE_TRANSFER_SEARCH_PLACEHOLDER = "Search for a cycle...";
/** Zero state when no open cycle can take the work. */
export const CYCLE_TRANSFER_NO_TARGET =
  "You don't have any current cycle. Please create one to transfer the work items.";
/** Zero state when the search narrows the open cycles to none. */
export const CYCLE_TRANSFER_NO_MATCH = "No cycles match your search.";

export interface CycleTransferDialogProps {
  workspaceSlug: string;
  projectId: string;
  sourceCycleId: string;
  open: boolean;
  onClose: () => void;
}

export function CycleTransferDialog({
  workspaceSlug,
  projectId,
  sourceCycleId,
  open,
  onClose,
}: CycleTransferDialogProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const cycles = usePickerCycles(workspaceSlug, projectId, open);
  const transfer = useTransferCycleWork({ workspaceSlug, projectId, sourceCycleId });
  const options = transferTargetOptions(cycles.data ?? [], sourceCycleId, query);

  React.useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  function pick(targetId: string): void {
    void transfer.transfer(targetId).then((moved) => {
      if (moved) onClose();
    });
  }

  return (
    <Dialog
      title={CYCLE_TRANSFER_TITLE}
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      nonDismissable={transfer.pending}
      actions={
        <Button variant="secondary" onClick={onClose} disabled={transfer.pending}>
          Cancel
        </Button>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={CYCLE_TRANSFER_SEARCH_PLACEHOLDER}
          aria-label="Search for a cycle"
          className="text-body h-9 w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
        />
        <div className="flex max-h-80 flex-col gap-(--space-1) overflow-y-auto" role="listbox" aria-label="Cycles">
          {cycles.isPending ? (
            <p className="text-meta p-(--space-3) text-(--text-muted)">Loading cycles…</p>
          ) : options.length === 0 ? (
            <p className="text-meta p-(--space-3) text-(--text-muted)">
              {query.trim() === "" ? CYCLE_TRANSFER_NO_TARGET : CYCLE_TRANSFER_NO_MATCH}
            </p>
          ) : (
            options.map((cycle) => (
              <button
                key={cycle.id}
                type="button"
                role="option"
                aria-selected={false}
                disabled={transfer.pending}
                onClick={() => pick(cycle.id)}
                className="text-body flex w-full items-center justify-between gap-(--space-3) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--subtle) disabled:opacity-60"
              >
                <span className="min-w-0 truncate">{cycle.name}</span>
                <span className="text-meta shrink-0 rounded-full bg-(--subtle) px-(--space-2) py-(--space-1) text-(--text-muted)">
                  {transferStatusPill(cycle.status)}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    </Dialog>
  );
}
