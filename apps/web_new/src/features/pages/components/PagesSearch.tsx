// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages title search (parity/specs/pages.md §3.2, PAGE-003). Collapsed
// until activated; typing filters the loaded rows immediately. Escape
// clears non-empty text and only collapses on the next press. Clicking
// outside collapses an empty control and leaves a filled one open.

import { IconButton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

export interface PagesSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
}

export const PAGES_SEARCH_PLACEHOLDER = "Search pages";

export function PagesSearch({ query, onQueryChange }: PagesSearchProps): React.ReactElement {
  const [expanded, setExpanded] = React.useState(query !== "");
  const containerRef = React.useRef<HTMLDivElement | null>(null);

  React.useEffect(() => {
    if (!expanded) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (event.target instanceof Element && containerRef.current?.contains(event.target) === true) return;
      if (query === "") setExpanded(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [expanded, query]);

  return (
    <div ref={containerRef} data-testid="pages-list-search" className="flex items-center">
      <IconButton
        label={expanded ? "Search pages" : "Open page search"}
        size="small"
        aria-expanded={expanded}
        onClick={() => setExpanded(true)}
      >
        <Search size={16} strokeWidth={1.5} aria-hidden="true" />
      </IconButton>
      {expanded ? (
        <>
          <input
            type="text"
            aria-label="Search pages"
            placeholder={PAGES_SEARCH_PLACEHOLDER}
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              if (query !== "") onQueryChange("");
              else setExpanded(false);
            }}
            className="text-body h-(--control-height) w-56 rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-3) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
          />
          <IconButton
            label="Clear page search"
            size="small"
            onClick={() => {
              onQueryChange("");
              setExpanded(false);
            }}
          >
            <X size={16} strokeWidth={1.5} aria-hidden="true" />
          </IconButton>
        </>
      ) : null}
    </div>
  );
}
