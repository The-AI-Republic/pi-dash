// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Live-list search box (parity/specs/cycles.md §3, CYC-005). A collapsed
// magnifier expands to a text field that filters rows as the user types;
// the field deliberately takes no focus on open (its input mounts only
// once open, unlike the archived box, whose input stays mounted and
// does take focus). Escape first clears the query and only collapses on
// a second press; the clear control empties and collapses at once; an
// outside click collapses an empty box but keeps a filled one. The
// query is remembered per project, so navigating away and back restores
// it into an open, unfocused box.

import { IconButton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

import { useCycleSearchStore } from "../api/searchStore.js";

export interface CyclesSearchBoxProps {
  projectId: string;
}

export const CYCLES_SEARCH_PLACEHOLDER = "Search";

export function CyclesSearchBox({ projectId }: CyclesSearchBoxProps): React.ReactElement {
  const query = useCycleSearchStore((state) => state.queryFor(projectId));
  const [open, setOpen] = React.useState(() => useCycleSearchStore.getState().queryFor(projectId) !== "");

  React.useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (event.target instanceof Element && event.target.closest('[data-testid="cycles-search-box"]') !== null) {
        return;
      }
      if (useCycleSearchStore.getState().queryFor(projectId) === "") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open, projectId]);

  if (!open) {
    return (
      <IconButton
        data-testid="cycles-search-toggle"
        label="Search cycles"
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
    if (query !== "") useCycleSearchStore.getState().clearQuery(projectId);
    else setOpen(false);
  };

  return (
    <div data-testid="cycles-search-box" role="search" className="flex items-center">
      <input
        aria-label="Search cycles"
        placeholder={CYCLES_SEARCH_PLACEHOLDER}
        value={query}
        onChange={(event) => {
          useCycleSearchStore.getState().setQuery(projectId, event.target.value);
        }}
        onKeyDown={onKeyDown}
        className="text-body h-(--control-height) w-56 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
      />
      <IconButton
        label="Close search"
        size="small"
        onClick={() => {
          useCycleSearchStore.getState().clearQuery(projectId);
          setOpen(false);
        }}
      >
        <X size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
    </div>
  );
}
