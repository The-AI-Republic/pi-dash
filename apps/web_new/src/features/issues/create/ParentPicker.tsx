// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parent picker for the create dialog (parity/specs/issues.md §11,
// ISS-122). An add-parent chip opens a project-scoped search that waits
// half a second after typing, lists matching work items with their state
// dot and an open-in-new-tab link, and swaps the chip for a parent tag
// (state dot, identifier, title, remove control) once one is picked. The
// search read lives in createApi.ts; this module owns only the popover
// and the chosen row.

import type { IssueSearchHit } from "@pidash/api-client";
import { Button, IconButton, Popover, cn } from "@pidash/kit";
import { ExternalLink, Loader2, Search, X } from "lucide-react";
import * as React from "react";

import { useDebouncedValue } from "../../../shared/commands/index.js";
import { PARENT_SEARCH_DEBOUNCE_MS, hitIdentifier, selectParentCandidates, useParentSearch } from "./createApi.js";

export interface ParentPickerProps {
  workspaceSlug: string;
  projectId: string;
  /** The chosen parent row, or null when the item has none. */
  value: IssueSearchHit | null;
  onChange: (parent: IssueSearchHit | null) => void;
  disabled?: boolean;
}

function stateDotColor(hit: IssueSearchHit): string {
  return hit.state__color ?? "var(--text-muted)";
}

/** Small circle in the state's color; the title repeats the state name. */
function StateDot({ hit }: { hit: IssueSearchHit }): React.ReactElement {
  return (
    <span
      aria-hidden="true"
      className="inline-block size-2 shrink-0 rounded-full"
      style={{ backgroundColor: stateDotColor(hit) }}
    />
  );
}

/** Browser-tab URL for one work item (the browse route's short key). */
export function parentBrowseHref(workspaceSlug: string, hit: IssueSearchHit): string {
  return `/${workspaceSlug}/browse/${hitIdentifier(hit)}`;
}

function stateLabel(hit: IssueSearchHit): string {
  return hit.state__name ?? "No state";
}

function CandidateRow({
  hit,
  workspaceSlug,
  onPick,
}: {
  hit: IssueSearchHit;
  workspaceSlug: string;
  onPick: () => void;
}): React.ReactElement {
  return (
    <li>
      <div className="flex items-center gap-(--space-2) rounded-(--radius-control) px-(--space-2) py-(--space-1) hover:bg-(--subtle)">
        <button
          type="button"
          onClick={onPick}
          className="flex min-w-0 flex-1 items-center gap-(--space-2) py-(--space-1) text-left"
        >
          <StateDot hit={hit} />
          <span className="text-meta shrink-0 text-(--text-muted)">{hitIdentifier(hit)}</span>
          <span className="text-body truncate text-(--text)">{hit.name}</span>
        </button>
        <a
          href={parentBrowseHref(workspaceSlug, hit)}
          target="_blank"
          rel="noreferrer"
          aria-label={`Open ${hitIdentifier(hit)} in a new tab`}
          className="text-(--text-muted) hover:text-(--text)"
        >
          <ExternalLink size={14} strokeWidth={1.5} aria-hidden="true" />
        </a>
      </div>
    </li>
  );
}

export function ParentPicker({
  workspaceSlug,
  projectId,
  value,
  onChange,
  disabled = false,
}: ParentPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const debounced = useDebouncedValue(query.trim(), PARENT_SEARCH_DEBOUNCE_MS);
  const search = useParentSearch(workspaceSlug, projectId, debounced, open && !disabled);
  const candidates = selectParentCandidates(search.data ?? [], value?.id ?? null);
  const loading = open && !disabled && (search.isPending || search.isFetching);

  function pick(hit: IssueSearchHit): void {
    onChange(hit);
    setOpen(false);
    setQuery("");
  }

  if (value !== null) {
    return (
      <span
        data-testid="parent-tag"
        className="inline-flex max-w-full items-center gap-(--space-2) rounded-(--radius-full) border border-(--border) bg-(--bg) py-(--space-1) pr-(--space-1) pl-(--space-3)"
      >
        <StateDot hit={value} />
        <span className="text-meta shrink-0 text-(--text-muted)">{hitIdentifier(value)}</span>
        <span className="text-body truncate text-(--text)">{value.name}</span>
        <span
          aria-label={`State: ${stateLabel(value)}`}
          title={stateLabel(value)}
          className="text-caption hidden truncate text-(--text-muted) sm:inline"
        >
          {stateLabel(value)}
        </span>
        <IconButton
          label={`Remove parent ${hitIdentifier(value)}`}
          size="small"
          disabled={disabled}
          onClick={() => onChange(null)}
        >
          <X size={12} strokeWidth={2} aria-hidden="true" />
        </IconButton>
      </span>
    );
  }

  return (
    <Popover
      trigger={
        <Button variant="secondary" disabled={disabled}>
          Add parent
        </Button>
      }
      title="Set a parent"
      description="Pick the work item this one breaks down from."
      open={open}
      onOpenChange={setOpen}
      className="w-80"
    >
      <div className="flex flex-col gap-(--space-2)">
        <div className="relative">
          <Search
            size={14}
            strokeWidth={1.5}
            aria-hidden="true"
            className="pointer-events-none absolute top-1/2 left-(--space-3) -translate-y-1/2 text-(--text-muted)"
          />
          <input
            type="search"
            aria-label="Search work items"
            placeholder="Search work items"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className={cn(
              "text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) pr-(--space-3) pl-(--space-8) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
            )}
          />
        </div>
        {loading ? (
          <p className="text-meta flex items-center gap-(--space-2) text-(--text-muted)">
            <Loader2 size={14} strokeWidth={1.5} aria-hidden="true" className="animate-spin" />
            Searching
          </p>
        ) : search.isError ? (
          <p className="text-meta text-(--danger)">Couldn't search work items.</p>
        ) : candidates.length === 0 ? (
          <p className="text-meta text-(--text-muted)">No matching work items.</p>
        ) : (
          <ul aria-label="Parent candidates" className="flex max-h-64 flex-col gap-px overflow-y-auto">
            {candidates.map((hit) => (
              <CandidateRow key={hit.id} hit={hit} workspaceSlug={workspaceSlug} onPick={() => pick(hit)} />
            ))}
          </ul>
        )}
      </div>
    </Popover>
  );
}
