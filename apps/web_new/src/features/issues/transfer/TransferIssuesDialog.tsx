// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Transfer-work-items dialog for a completed cycle (parity/specs/issues.md
// §18, ISS-224). A search box narrows the incomplete cycles; picking one
// transfers that cycle's remaining work and closes. The source cycle is
// never offered. A refused transfer notices through the mutation and
// leaves the dialog open so the reader can try another target.

import { Button, Dialog } from "@pidash/kit";
import { RefreshCw } from "lucide-react";
import * as React from "react";

import { usePickerCycles } from "../../../shared/pickers/reference.js";
import { useTransferCycleIssues } from "./api.js";
import { transferTargetOptions } from "./rules.js";

/** Search placeholder the oracle looks for. */
export const TRANSFER_SEARCH_PLACEHOLDER = "Search for a cycle...";

export interface TransferIssuesDialogProps {
  workspaceSlug: string;
  projectId: string;
  cycleId: string;
  open: boolean;
  onClose: () => void;
}

export function TransferIssuesDialog({
  workspaceSlug,
  projectId,
  cycleId,
  open,
  onClose,
}: TransferIssuesDialogProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const cycles = usePickerCycles(workspaceSlug, projectId, open);
  const transfer = useTransferCycleIssues(workspaceSlug, projectId, cycleId);
  const options = transferTargetOptions(cycles.data ?? [], cycleId, query);

  React.useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  function pick(targetId: string): void {
    transfer.mutate(targetId, { onSuccess: onClose });
  }

  return (
    <Dialog
      title="Transfer work items"
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      nonDismissable={transfer.isPending}
      actions={
        <Button variant="secondary" onClick={onClose} disabled={transfer.isPending}>
          Cancel
        </Button>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={TRANSFER_SEARCH_PLACEHOLDER}
          aria-label="Search for a cycle"
          className="text-body h-9 w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) outline-none focus-visible:ring-2 focus-visible:ring-(--accent)"
        />
        <div className="flex max-h-80 flex-col gap-(--space-1) overflow-y-auto" role="listbox" aria-label="Cycles">
          {cycles.isPending ? (
            <p className="text-meta p-(--space-3) text-(--text-muted)">Loading cycles…</p>
          ) : options.length === 0 ? (
            <p className="text-meta p-(--space-3) text-(--text-muted)">No incomplete cycle to move the work into.</p>
          ) : (
            options.map((cycle) => (
              <button
                key={cycle.id}
                type="button"
                role="option"
                aria-selected={false}
                disabled={transfer.isPending}
                onClick={() => pick(cycle.id)}
                className="text-body flex w-full items-center justify-between gap-(--space-3) rounded-(--radius-control) px-(--space-3) py-(--space-2) text-left text-(--text) hover:bg-(--surface-raised) disabled:opacity-60"
              >
                <span className="flex min-w-0 items-center gap-(--space-2)">
                  <RefreshCw size={16} strokeWidth={1.5} aria-hidden="true" />
                  <span className="truncate">{cycle.name}</span>
                </span>
                {cycle.status ? (
                  <span className="text-meta shrink-0 rounded-full bg-(--surface-raised) px-(--space-2) text-(--text-muted) lowercase">
                    {cycle.status.toLowerCase()}
                  </span>
                ) : null}
              </button>
            ))
          )}
        </div>
      </div>
    </Dialog>
  );
}
