// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Duplicate-target picker (parity/specs/intake.md INT-023). A
// project-scoped work-item search — recent rows on an empty query,
// matches once the viewer types, never the acted request itself.
// Picking a row marks the request duplicate at once; there is no
// second confirmation. The caller owns the write and closes on settle.

import type { IssueSearchHit } from "@pidash/api-client";
import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

import { useDebouncedValue } from "../../../shared/commands/index.js";
import { excludeCurrentTarget, useDuplicateTargetSearch } from "../api/triage.js";

/** Duplicate dialog heading. */
export const DUPLICATE_DIALOG_TITLE = "Mark as duplicate";
/** Accessible name of the picker search field. */
export const DUPLICATE_SEARCH_LABEL = "Search work items";
/** Pause after typing before the search read fires. */
export const DUPLICATE_SEARCH_DEBOUNCE_MS = 500;

export interface IntakeDuplicateDialogProps {
  open: boolean;
  /** A pick is being written; rows lock. */
  pending: boolean;
  workspaceSlug: string;
  projectId: string;
  /** The acted request's work-item id, excluded from the results. */
  currentIssueId: string;
  onCancel: () => void;
  onPick: (targetId: string) => void;
}

function targetKey(hit: IssueSearchHit): string {
  return `${hit.project__identifier}-${hit.sequence_id}`;
}

export function IntakeDuplicateDialog({
  open,
  pending,
  workspaceSlug,
  projectId,
  currentIssueId,
  onCancel,
  onPick,
}: IntakeDuplicateDialogProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const debounced = useDebouncedValue(query.trim(), DUPLICATE_SEARCH_DEBOUNCE_MS);
  const search = useDuplicateTargetSearch(workspaceSlug, projectId, debounced, open);
  const candidates = excludeCurrentTarget(search.data ?? [], currentIssueId);
  React.useEffect(() => {
    if (open) setQuery("");
  }, [open]);

  return (
    <Dialog
      title={DUPLICATE_DIALOG_TITLE}
      description="Search the project's work items for the original request."
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      actions={
        <Button variant="secondary" onClick={onCancel} disabled={pending}>
          Cancel
        </Button>
      }
    >
      <div className="flex flex-col gap-(--space-3)">
        <label className="text-meta font-medium text-(--text-muted)" htmlFor="intake-duplicate-search">
          {DUPLICATE_SEARCH_LABEL}
        </label>
        <input
          id="intake-duplicate-search"
          type="search"
          value={query}
          placeholder="Type to search"
          disabled={pending}
          onChange={(event) => setQuery(event.target.value)}
          className="text-body min-w-0 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) py-(--space-2) text-(--text)"
        />
        {search.isPending ? (
          <p className="text-body text-(--text-muted)">Searching work items…</p>
        ) : search.isError ? (
          <div className="flex items-center gap-(--space-3)">
            <p role="alert" className="text-body text-(--danger)">
              Couldn&apos;t search work items.
            </p>
            <Button variant="secondary" size="small" onClick={() => void search.refetch()}>
              Retry
            </Button>
          </div>
        ) : candidates.length === 0 ? (
          <p className="text-body text-(--text-muted)">No work items found.</p>
        ) : (
          <ul className="flex max-h-64 flex-col overflow-y-auto">
            {candidates.map((hit) => (
              <li key={hit.id}>
                <button
                  type="button"
                  data-testid="intake-duplicate-option"
                  disabled={pending}
                  onClick={() => onPick(hit.id)}
                  className="flex w-full min-w-0 items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left hover:bg-(--subtle) disabled:opacity-50"
                >
                  <span className="text-meta shrink-0 text-(--text-muted)">{targetKey(hit)}</span>
                  <span className="text-body truncate text-(--text)">{hit.name}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
