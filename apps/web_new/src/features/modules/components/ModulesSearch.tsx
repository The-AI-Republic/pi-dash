// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Expanding name search for the modules header (parity/specs/modules.md
// §2.4, MOD-004). Collapsed it is one icon button; activating it opens the
// field and focuses it in the same interaction. Escape clears a filled
// field and keeps focus, then collapses an empty one on the next press. A
// press outside collapses only an empty field — a filled query stays open
// so its narrowed list remains visible. Search never leaves the browser.

import { IconButton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

export const MODULES_SEARCH_TRIGGER_LABEL = "Search modules";

export interface ModulesSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
}

export function ModulesSearch({ query, onQueryChange }: ModulesSearchProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const restoreFocus = React.useRef(false);
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const inputRef = React.useRef<HTMLInputElement | null>(null);
  const expanded = open || query !== "";

  // A query restored from the project's record keeps the field open.
  React.useEffect(() => {
    if (query !== "") setOpen(true);
  }, [query]);

  React.useEffect(() => {
    if (open) {
      inputRef.current?.focus();
    } else if (restoreFocus.current) {
      restoreFocus.current = false;
      containerRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    }
  }, [open]);

  React.useEffect(() => {
    if (!expanded) return undefined;
    const collapseOutside = (event: Event): void => {
      if (containerRef.current?.contains(event.target as Node) === true) return;
      if (query !== "") return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", collapseOutside);
    return () => document.removeEventListener("pointerdown", collapseOutside);
  }, [expanded, query]);

  function collapse(): void {
    restoreFocus.current = true;
    setOpen(false);
  }

  function onKeyDown(event: React.KeyboardEvent<HTMLInputElement>): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (query !== "") {
      onQueryChange("");
      return;
    }
    collapse();
  }

  return (
    <div ref={containerRef} data-testid="modules-search" className="flex items-center">
      {expanded ? (
        <div
          data-testid="modules-search-field"
          className="flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1)"
        >
          <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            ref={inputRef}
            type="text"
            aria-label="Search modules by name"
            placeholder="Search"
            value={query}
            data-testid="modules-search-input"
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
        <IconButton label={MODULES_SEARCH_TRIGGER_LABEL} onClick={() => setOpen(true)}>
          <Search size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      )}
    </div>
  );
}
