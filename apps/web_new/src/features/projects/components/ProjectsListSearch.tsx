// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Expanding projects-list search (SHELL-031). A magnifier opens the
// field; typing narrows live, the first escape clears the text, the
// second collapses, an outside click collapses only a blank field, and
// the clear button resets and collapses. Written from
// parity/specs/shell.md §3.

import { IconButton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

export interface ProjectsListSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
}

export function ProjectsListSearch({ query, onQueryChange }: ProjectsListSearchProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const inputRef = React.useRef<HTMLInputElement | null>(null);

  React.useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  React.useEffect(() => {
    if (!open) return;
    const collapseOnOutside = (event: PointerEvent): void => {
      if (containerRef.current?.contains(event.target as Node) !== false) return;
      if (query.trim() !== "") return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", collapseOnOutside);
    return () => document.removeEventListener("pointerdown", collapseOnOutside);
  }, [open, query]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>): void => {
    if (event.key !== "Escape") return;
    if (query.trim() !== "") onQueryChange("");
    else setOpen(false);
  };

  return (
    <div ref={containerRef} data-testid="projects-list-search" className="flex items-center">
      {open ? (
        <div className="flex items-center gap-(--space-1) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1)">
          <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
          <input
            ref={inputRef}
            type="text"
            aria-label="Search projects"
            placeholder="Search"
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={onKeyDown}
            className="text-body w-40 bg-transparent outline-none placeholder:text-(--text-muted)"
          />
          <IconButton
            label="Clear search"
            size="small"
            onClick={() => {
              onQueryChange("");
              setOpen(false);
            }}
          >
            <X size={14} strokeWidth={2} aria-hidden="true" />
          </IconButton>
        </div>
      ) : (
        <IconButton label="Search projects" onClick={() => setOpen(true)}>
          <Search size={16} strokeWidth={1.5} aria-hidden="true" />
        </IconButton>
      )}
    </div>
  );
}
