// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sibling-page switcher (parity/specs/pages.md §4.3, PAGE-024/080). The
// trigger opens a searchable list of the other pages the collection holds
// (never archived, never the page already open); picking one navigates to
// its canonical address, dropping the pane-tab and version keys.

import { ChevronDown } from "lucide-react";
import * as React from "react";

import { filterSiblingOptions, type PageSiblingOption } from "../lib/pageDetail.js";
import { PageMarkGlyph } from "./PageMarkButton.js";

export const PAGE_SWITCHER_LABEL = "Switch page";
export const PAGE_SWITCHER_SEARCH = "Search";
export const PAGE_SWITCHER_EMPTY = "No matching pages";

export interface PageSiblingSwitcherProps {
  currentName: string;
  /** The open page's stored mark, drawn on the trigger (PAGE-027). */
  currentLogoProps?: Record<string, unknown> | undefined;
  options: readonly PageSiblingOption[];
  /** Navigate to the picked page's canonical address. */
  onSelect: (pageId: string) => void;
}

export function PageSiblingSwitcher({
  currentName,
  currentLogoProps = {},
  options,
  onSelect,
}: PageSiblingSwitcherProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [query, setQuery] = React.useState("");
  const rootRef = React.useRef<HTMLDivElement>(null);
  const searchRef = React.useRef<HTMLInputElement>(null);
  const listId = React.useId();
  const visible = filterSiblingOptions(options, query);

  React.useEffect(() => {
    if (!open) return;
    searchRef.current?.focus();
    function onPointerDown(event: MouseEvent): void {
      if (rootRef.current !== null && !rootRef.current.contains(event.target as Node)) close();
    }
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key === "Escape") close();
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  function close(): void {
    setOpen(false);
    setQuery("");
  }

  return (
    <div ref={rootRef} className="relative min-w-0">
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-label={`${PAGE_SWITCHER_LABEL}, current ${currentName}`}
        onClick={() => {
          if (open) close();
          else setOpen(true);
        }}
        className="text-body flex max-w-56 min-w-0 cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) px-(--space-1) py-(--space-1) text-(--text) hover:bg-(--subtle)"
      >
        <span
          data-testid="page-switcher-current-mark"
          className="inline-flex size-4 shrink-0 items-center justify-center"
        >
          <PageMarkGlyph logoProps={currentLogoProps} />
        </span>
        <span data-testid="page-switcher-current-name" className="truncate">
          {currentName}
        </span>
        <ChevronDown size={14} strokeWidth={1.5} aria-hidden="true" className="shrink-0 text-(--text-muted)" />
      </button>
      {open ? (
        <div className="shadow-popover absolute top-full left-0 z-20 mt-(--space-1) w-64 rounded-(--radius-popover) border border-(--border) bg-(--bg) p-(--space-2)">
          <input
            ref={searchRef}
            type="search"
            aria-label={PAGE_SWITCHER_SEARCH}
            placeholder={PAGE_SWITCHER_SEARCH}
            autoComplete="off"
            spellCheck={false}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
            }}
            className="text-body mb-(--space-2) h-8 w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) text-(--text)"
          />
          {visible.length === 0 ? (
            <p role="status" className="text-body px-(--space-2) py-(--space-3) text-(--text-muted)">
              {PAGE_SWITCHER_EMPTY}
            </p>
          ) : (
            <ul id={listId} role="listbox" aria-label="Pages" className="max-h-64 overflow-y-auto">
              {visible.map((option) => (
                <li key={option.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={false}
                    onClick={() => {
                      close();
                      onSelect(option.id);
                    }}
                    className="text-body flex w-full cursor-pointer items-center gap-(--space-1) rounded-(--radius-control) px-(--space-2) py-(--space-2) text-left text-(--text) hover:bg-(--subtle)"
                  >
                    <span
                      data-testid="page-switcher-mark"
                      className="inline-flex size-4 shrink-0 items-center justify-center"
                    >
                      <PageMarkGlyph logoProps={option.logoProps} />
                    </span>
                    <span data-testid="page-switcher-option-name" className="truncate">
                      {option.name}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}
