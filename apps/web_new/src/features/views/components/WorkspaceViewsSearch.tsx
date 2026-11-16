// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Name search above the workspace views list (parity/specs/views.md §9,
// VIEW-029). One always-visible field: typing narrows both groups live,
// Escape clears the text, and the clear button does the same with a click.
// The field is the page's only list control — this surface has no sort,
// filter or creation affordance.

import { IconButton } from "@pidash/kit";
import { Search, X } from "lucide-react";
import * as React from "react";

export interface WorkspaceViewsSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
}

export function WorkspaceViewsSearch({ query, onQueryChange }: WorkspaceViewsSearchProps): React.ReactElement {
  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>): void => {
    if (event.key !== "Escape") return;
    onQueryChange("");
  };

  return (
    <div
      data-testid="workspace-views-search"
      className="flex items-center gap-(--space-2) border-b border-(--border) px-(--space-4) py-(--space-2)"
    >
      <Search size={14} strokeWidth={2} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
      <input
        type="text"
        aria-label="Search views"
        placeholder="Search"
        value={query}
        onChange={(event) => onQueryChange(event.target.value)}
        onKeyDown={onKeyDown}
        className="text-body min-w-0 flex-1 bg-transparent outline-none placeholder:text-(--text-muted)"
      />
      {query === "" ? null : (
        <IconButton label="Clear search" size="small" onClick={() => onQueryChange("")}>
          <X size={14} strokeWidth={2} aria-hidden="true" />
        </IconButton>
      )}
    </div>
  );
}
