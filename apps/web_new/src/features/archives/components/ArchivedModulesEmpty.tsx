// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The archived-modules screen's three settled empties
// (parity/specs/modules.md §7.3, MOD-027). Each names its own cause so the
// viewer knows what to change: an archive with nothing in it yet, a name
// search that hides every module, or filters that do. The two narrowing
// empties carry the control that undoes them; the zero state explains how
// modules reach the screen. The kind is exposed as data because the oracle
// reads which empty is showing, and the bodies never share a marker.

import { emptyStateImagePath } from "@pidash/kit";
import { ListFilter, Search } from "lucide-react";
import * as React from "react";

/** Which settled empty the screen shows. */
export type ArchivedModulesEmptyKind = "zero" | "search" | "filters";

export const ARCHIVED_MODULES_ZERO_HEADING = "No archived modules yet";
export const ARCHIVED_MODULES_ZERO_BODY =
  "Completed and cancelled modules you archive are kept here. Archiving clears a module from the active list without deleting it, and you can restore it from this screen.";

export const ARCHIVED_MODULES_SEARCH_HEADING = "No modules match your search";
export const ARCHIVED_MODULES_SEARCH_BODY =
  "No archived module carries that name. Change the search or clear it to see the whole archive again.";

export const ARCHIVED_MODULES_FILTERS_HEADING = "No modules match the filters";
export const ARCHIVED_MODULES_FILTERS_BODY =
  "No archived module matches every applied filter. Remove a filter or clear them all to see the whole archive again.";

export interface ArchivedModulesEmptyProps {
  kind: ArchivedModulesEmptyKind;
  onClearSearch?: (() => void) | undefined;
  onClearFilters?: (() => void) | undefined;
}

export function ArchivedModulesEmpty({
  kind,
  onClearSearch,
  onClearFilters,
}: ArchivedModulesEmptyProps): React.ReactElement {
  if (kind === "zero") {
    return (
      <div
        data-testid="archived-modules-empty"
        data-empty-kind="zero"
        className="flex flex-col items-center gap-(--space-4) px-(--space-4) py-(--space-12) text-center"
      >
        <img
          src={emptyStateImagePath({ category: "general", type: "empty" })}
          alt=""
          aria-hidden="true"
          draggable={false}
          className="h-40 w-40"
        />
        <h3 data-testid="archived-modules-empty-heading" className="text-h3 text-(--text)">
          {ARCHIVED_MODULES_ZERO_HEADING}
        </h3>
        <p className="text-body max-w-96 text-(--text-muted)">{ARCHIVED_MODULES_ZERO_BODY}</p>
      </div>
    );
  }

  const search = kind === "search";
  return (
    <div
      data-testid="archived-modules-empty"
      data-empty-kind={kind}
      className="flex flex-col items-center gap-(--space-4) px-(--space-4) py-(--space-12) text-center"
    >
      <span
        aria-hidden="true"
        className="grid size-12 place-items-center rounded-full border border-(--border) bg-(--subtle) text-(--text-muted)"
      >
        {search ? <Search size={20} strokeWidth={1.5} /> : <ListFilter size={20} strokeWidth={1.5} />}
      </span>
      <h3 data-testid="archived-modules-empty-heading" className="text-h3 text-(--text)">
        {search ? ARCHIVED_MODULES_SEARCH_HEADING : ARCHIVED_MODULES_FILTERS_HEADING}
      </h3>
      <p className="text-body max-w-96 text-(--text-muted)">
        {search ? ARCHIVED_MODULES_SEARCH_BODY : ARCHIVED_MODULES_FILTERS_BODY}
      </p>
      {search ? (
        <button
          type="button"
          data-testid="archived-modules-empty-clear-search"
          onClick={onClearSearch}
          className="text-body cursor-pointer rounded-(--radius-control) font-medium text-(--accent) hover:underline"
        >
          Clear search
        </button>
      ) : (
        <button
          type="button"
          data-testid="archived-modules-empty-clear-filters"
          onClick={onClearFilters}
          className="text-body cursor-pointer rounded-(--radius-control) font-medium text-(--accent) hover:underline"
        >
          Clear filters
        </button>
      )}
    </div>
  );
}
