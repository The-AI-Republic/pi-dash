// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-existing sub-work items (parity/specs/issues.md §15, ISS-175). One
// project-scoped search over the same project — there is no workspace
// toggle here — whose rows can be ticked several at a time, the picked
// rows listed above the list, and one submit that posts every ticked id
// to the parent's sub-issues collection. Making a pick leaves the dialog
// open so a parent can take more than one child in a visit.

import { Badge, Button, Dialog } from "@pidash/kit";
import type { IssueSearchHit } from "@pidash/api-client";
import { Check, Search } from "lucide-react";
import * as React from "react";

import { useDebouncedValue } from "../../../shared/commands/index.js";
import { PARENT_SEARCH_DEBOUNCE_MS } from "../create/createApi.js";
import { useSubIssueSearch } from "./queries.js";
import { useAttachSubIssues } from "./mutations.js";
import type { SubIssueScope } from "./queries.js";

/** Debounce shared with the parent picker. */
export const ADD_EXISTING_DEBOUNCE_MS = PARENT_SEARCH_DEBOUNCE_MS;

/** The identifier a search hit renders. */
export function hitIdentifier(hit: IssueSearchHit): string {
  return `${hit.project__identifier}-${hit.sequence_id}`;
}

/** Hits that are not already a child of this parent, picked kept first. */
export function selectAddCandidates(
  hits: readonly IssueSearchHit[],
  picked: readonly string[],
  parentId: string
): IssueSearchHit[] {
  const seen = new Set<string>();
  const rows: IssueSearchHit[] = [];
  for (const hit of hits) {
    if (hit.id === parentId || seen.has(hit.id)) continue;
    seen.add(hit.id);
    rows.push(hit);
  }
  return rows.sort((left, right) => {
    const leftPicked = picked.includes(left.id) ? 0 : 1;
    const rightPicked = picked.includes(right.id) ? 0 : 1;
    return leftPicked - rightPicked;
  });
}

/** Toggle one id in a ticked set, preserving pick order. */
export function togglePicked(picked: readonly string[], id: string): string[] {
  return picked.includes(id) ? picked.filter((entry) => entry !== id) : [...picked, id];
}

/** A state dot in the hit's own color, muted when the row has no state. */
function StateDot({ hit }: { hit: IssueSearchHit }): React.ReactElement {
  return (
    <span
      aria-hidden="true"
      className="inline-block size-2 shrink-0 rounded-full"
      style={{ backgroundColor: hit.state__color ?? "var(--text-muted)" }}
    />
  );
}

export interface AddExistingDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  scope: SubIssueScope;
  /** Short code of the parent's project, for the modal's heading. */
  projectName: string;
}

export function AddExistingDialog({
  open,
  onOpenChange,
  scope,
  projectName,
}: AddExistingDialogProps): React.ReactElement {
  const [query, setQuery] = React.useState("");
  const [picked, setPicked] = React.useState<string[]>([]);
  const debounced = useDebouncedValue(query.trim(), ADD_EXISTING_DEBOUNCE_MS);
  const search = useSubIssueSearch(scope.workspaceSlug, scope.projectId, debounced, open);
  const attach = useAttachSubIssues(scope);
  const candidates = selectAddCandidates(search.data ?? [], picked, scope.parentId);
  const loading = open && (search.isPending || search.isFetching);
  const pickedHits = picked
    .map((id) => candidates.find((hit) => hit.id === id))
    .filter((hit): hit is IssueSearchHit => hit !== undefined);

  function close(): void {
    setQuery("");
    setPicked([]);
    onOpenChange(false);
  }

  function submit(): void {
    // The submit stays disabled at zero picks, so this always has work.
    attach.mutate(picked, { onSuccess: () => close() });
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) close();
      }}
      title="Add existing work items"
      description={`Pick work items from ${projectName} to place under this one.`}
      className="max-w-lg"
      actions={
        <>
          <Button variant="ghost" onClick={close} disabled={attach.isPending}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={submit}
            disabled={attach.isPending || picked.length === 0}
            aria-label="Add selected work items"
          >
            {attach.isPending ? "Adding…" : `Add selected work items (${picked.length})`}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-3)" data-testid="add-existing-dialog">
        <label className="flex items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-2)">
          <Search size={14} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
          <input
            type="search"
            aria-label="Search work items"
            placeholder="Search work items"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className="text-body h-8 min-w-0 flex-1 bg-transparent text-(--text) outline-none"
          />
        </label>
        {pickedHits.length > 0 ? (
          <ul aria-label="Selected work items" className="flex flex-wrap gap-(--space-1)">
            {pickedHits.map((hit) => (
              <li key={hit.id}>
                <button
                  type="button"
                  aria-label={`Unpick ${hitIdentifier(hit)}`}
                  onClick={() => setPicked((current) => togglePicked(current, hit.id))}
                  className="text-caption inline-flex items-center gap-(--space-1) rounded-(--radius-full) border border-(--border) px-(--space-2) py-px text-(--text)"
                >
                  <StateDot hit={hit} />
                  {hitIdentifier(hit)}
                  <Check size={12} strokeWidth={2} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        ) : null}
        {search.isError ? (
          <p role="alert" className="text-body text-(--danger)">
            Couldn&apos;t search this project. Try again.
          </p>
        ) : loading ? (
          <p role="status" className="text-body text-(--text-muted)">
            Searching…
          </p>
        ) : candidates.length === 0 ? (
          <p role="status" className="text-body text-(--text-muted)">
            No work items match this search.
          </p>
        ) : (
          <ul
            role="listbox"
            aria-label="Work items to add"
            aria-multiselectable="true"
            className="max-h-64 overflow-y-auto"
          >
            {candidates.map((hit) => {
              const selected = picked.includes(hit.id);
              return (
                <li key={hit.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={selected}
                    onClick={() => setPicked((current) => togglePicked(current, hit.id))}
                    className="text-body flex w-full items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) text-left text-(--text) hover:bg-(--subtle)"
                  >
                    <span className="flex size-4 shrink-0 items-center justify-center">
                      {selected ? <Check size={14} strokeWidth={2} aria-hidden="true" /> : null}
                    </span>
                    <StateDot hit={hit} />
                    <span className="shrink-0 font-[family-name:var(--font-mono)] text-(--text-muted)">
                      {hitIdentifier(hit)}
                    </span>
                    <span className="min-w-0 flex-1 truncate">{hit.name}</span>
                    {hit.state__name === null ? null : <Badge tone="neutral">{hit.state__name}</Badge>}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
