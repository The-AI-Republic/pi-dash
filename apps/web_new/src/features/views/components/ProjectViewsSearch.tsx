// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Expanding name search for the project views list (parity/specs/views.md
// §4, VIEW-004). Collapsed it is a single icon button; opening expands the
// field and focuses it in the same interaction. Escape clears a non-empty
// query and collapses only an empty one, returning focus to the trigger; a
// press anywhere outside collapses an empty field and leaves a filtered
// result on screen. The collapsed control exposes no value and nothing to
// focus but its own trigger.

import { IconButton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

export interface ProjectViewsSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
}

export const VIEWS_SEARCH_TRIGGER_LABEL = "Search views";

export function ProjectViewsSearch({ query, onQueryChange }: ProjectViewsSearchProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const restoreFocus = React.useRef(false);
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const inputRef = React.useRef<HTMLInputElement | null>(null);
  const expanded = open || query !== "";

  // A query that arrived from elsewhere (a restored visit, the clear
  // button of a host that owns the text) keeps the field expanded: only
  // Escape, the clear control and an outside press collapse it.
  React.useEffect(() => {
    if (query !== "") setOpen(true);
  }, [query]);

  React.useEffect(() => {
    if (open) inputRef.current?.focus();
    else if (restoreFocus.current) {
      restoreFocus.current = false;
      containerRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    }
  }, [open]);

  React.useEffect(() => {
    if (!expanded) return undefined;
    const collapseOutside = (event: Event): void => {
      if (containerRef.current?.contains(event.target as Node) === true) return;
      // A query keeps the field open so its result stays visible.
      if (query !== "") return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", collapseOutside);
    return () => document.removeEventListener("pointerdown", collapseOutside);
  }, [expanded, query]);

  const collapse = (): void => {
    restoreFocus.current = true;
    setOpen(false);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>): void => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (query !== "") {
      onQueryChange("");
      return;
    }
    collapse();
  };

  return (
    <div ref={containerRef} data-testid="project-views-search" className="flex items-center">
      {expanded ? (
        <div
          data-search-expanded="true"
          className="flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1)"
        >
          <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            ref={inputRef}
            type="text"
            aria-label="Search views by name"
            placeholder="Search"
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={onKeyDown}
            className="text-body w-40 bg-transparent text-(--text) outline-none placeholder:text-(--text-muted)"
          />
          <IconButton
            label="Clear search"
            size="small"
            onClick={() => {
              onQueryChange("");
              collapse();
            }}
          >
            <X size={14} strokeWidth={2} aria-hidden="true" />
          </IconButton>
        </div>
      ) : (
        <IconButton label={VIEWS_SEARCH_TRIGGER_LABEL} onClick={() => setOpen(true)}>
          <Search size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      )}
    </div>
  );
}
