// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archived-cycles search box (parity/specs/archives.md §4.2,
// ARCH-015). A collapsed magnifier expands to a text field that takes
// focus and filters the loaded rows live as the user types; it never
// issues a request. Escape first clears the query and only collapses
// on a second press; the clear control empties and collapses at once;
// an outside click collapses an empty box but keeps a filled one. The
// query is remembered in the archived slot, so navigating away and
// back restores it into an open box.

import { IconButton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

// Deep import: the archived-cycles route is eager (size gate).
import { useCycleSearchStore } from "../../cycles/api/searchStore.js";
import { archivedCycleStoreSlot } from "../api/archivedCycles.js";

export interface ArchivedCyclesSearchBoxProps {
  projectId: string;
}

/** Accessible name shared by the toggle and the expanded input. */
export const ARCHIVED_CYCLES_SEARCH_LABEL = "Search archived cycles";

export function ArchivedCyclesSearchBox({ projectId }: ArchivedCyclesSearchBoxProps): React.ReactElement {
  const slot = archivedCycleStoreSlot(projectId);
  const query = useCycleSearchStore((state) => state.queryFor(slot));
  const [open, setOpen] = React.useState(() => useCycleSearchStore.getState().queryFor(slot) !== "");

  React.useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (
        event.target instanceof Element &&
        event.target.closest('[data-testid="archived-cycles-search-box"]') !== null
      ) {
        return;
      }
      if (useCycleSearchStore.getState().queryFor(slot) === "") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open, slot]);

  if (!open) {
    return (
      <IconButton
        data-testid="archived-cycles-search-toggle"
        label={ARCHIVED_CYCLES_SEARCH_LABEL}
        size="small"
        onClick={() => {
          setOpen(true);
        }}
      >
        <Search size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
    );
  }

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>): void => {
    if (event.key !== "Escape") return;
    if (query !== "") useCycleSearchStore.getState().clearQuery(slot);
    else setOpen(false);
  };

  return (
    <div data-testid="archived-cycles-search-box" role="search" className="flex items-center">
      <input
        data-testid="archived-cycles-search-input"
        aria-label={ARCHIVED_CYCLES_SEARCH_LABEL}
        placeholder="Search"
        autoFocus
        value={query}
        onChange={(event) => {
          useCycleSearchStore.getState().setQuery(slot, event.target.value);
        }}
        onKeyDown={onKeyDown}
        className="text-body h-(--control-height) w-56 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
      />
      <IconButton
        data-testid="archived-cycles-search-clear"
        label="Clear search"
        size="small"
        onClick={() => {
          useCycleSearchStore.getState().clearQuery(slot);
          setOpen(false);
        }}
      >
        <X size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
    </div>
  );
}
