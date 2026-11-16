// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parent picker (parity/specs/issues.md §13, ISS-157). A dialog with a
// search field over the project's work items; the search excludes the
// item itself, the item's current parent and its own children, so a pick
// can never close a loop. Results carry the state dot, the identifier and
// the name. Picking reports the id and closes; an empty answer names the
// query. Results settle behind a short debounce.

import { Dialog, Input, Skeleton } from "@pidash/kit";
import { searchIssues, type IssueSearchHit } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../../core/api/client.js";
import { useDebouncedValue } from "../../../../shared/commands/index.js";
import { issueKeys } from "../../api/keys.js";

/** Quiet period before the search request follows the typed text. */
export const PARENT_SEARCH_DEBOUNCE_MS = 150;

/** The project's work items eligible to become this item's parent. */
export function parentSearchQueryOptions(
  workspaceSlug: string,
  projectId: string,
  issueId: string,
  search: string,
  enabled: boolean
) {
  return {
    queryKey: [...issueKeys.all(workspaceSlug), "parent-search", projectId, issueId, search] as const,
    queryFn: () => searchIssues(getAppClient(), workspaceSlug, projectId, { search, parent: true, issueId }),
    enabled,
  };
}

export interface ParentPickerDialogProps {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (parentId: string) => void;
}

export function ParentPickerDialog({
  workspaceSlug,
  projectId,
  issueId,
  open,
  onOpenChange,
  onPick,
}: ParentPickerDialogProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const debounced = useDebouncedValue(query, PARENT_SEARCH_DEBOUNCE_MS);
  const results = useQuery(parentSearchQueryOptions(workspaceSlug, projectId, issueId, debounced, open));

  React.useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  const hits: IssueSearchHit[] = results.data ?? [];

  function pick(hit: IssueSearchHit): void {
    onPick(hit.id);
    onOpenChange(false);
  }

  return (
    <Dialog
      title="Set parent work item"
      open={open}
      onOpenChange={(next) => {
        onOpenChange(next);
      }}
    >
      <div className="flex flex-col gap-(--space-3)">
        <Input
          label="Search work items"
          placeholder="Type to search"
          value={query}
          autoFocus
          autoComplete="off"
          onChange={(event) => {
            setQuery(event.target.value);
          }}
        />
        {results.isError ? (
          <p role="alert" className="text-body text-(--danger)">
            Couldn't search the work items.
          </p>
        ) : results.isPending ? (
          <div role="status" aria-label="Searching work items" className="flex flex-col gap-(--space-2)">
            <Skeleton height="32px" />
            <Skeleton height="32px" />
          </div>
        ) : hits.length === 0 ? (
          <p role="status" className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">
            No matching work items
          </p>
        ) : (
          <ul aria-label="Work items" className="flex max-h-72 flex-col gap-px overflow-y-auto">
            {hits.map((hit) => (
              <li key={hit.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={false}
                  onClick={() => {
                    pick(hit);
                  }}
                  className="flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left hover:bg-(--subtle)"
                >
                  <span
                    aria-hidden="true"
                    style={hit.state__color === null ? undefined : { backgroundColor: hit.state__color }}
                    className="size-2 shrink-0 rounded-full bg-(--text-muted)"
                  />
                  <span className="text-caption shrink-0 text-(--text-muted)">
                    {`${hit.project__identifier}-${hit.sequence_id}`}
                  </span>
                  <span className="text-body min-w-0 flex-1 truncate text-(--text)">{hit.name}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
