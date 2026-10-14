// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Top-bar search box (shell spec, SHELL-081). An always-visible field that
// opens an inline (non-dialog) results panel on focus: the same commands
// plus grouped server hits as the modal palette. Escape clears the term and
// closes in one press; an outside click closes; closing always resets the
// term. Opening the modal palette resets this panel with it.
// DOM contract for the area-gate driver (NEWFRONT-295): a textbox with
// placeholder TOPBAR_SEARCH_PLACEHOLDER; the open panel is role=listbox
// named "Top bar search results" (never a dialog); rows are role=option
// with a single aria-selected=true; the results heading pulses mid-flight.

import { useQuery } from "@tanstack/react-query";
import { Spinner } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../core/api/index.js";
import {
  PALETTE_SEARCH_DEBOUNCE_MS,
  countSearchHits,
  filterCommands,
  groupSearchResults,
  listCommands,
  paletteSearchQueryOptions,
  useCommandPalette,
  useDebouncedValue,
  type PaletteSearchScope,
  type SearchResultGroup,
  type SearchResultHit,
} from "../commands/index.js";
import { usePaletteKeys } from "./paletteKeys.js";
import { groupCommands, PaletteOption, type PaletteRow } from "./paletteRows.js";

/** The always-visible top-bar field prompt (SHELL-081). */
export const TOPBAR_SEARCH_PLACEHOLDER = "Search commands...";

export interface TopBarSearchProps {
  /** Null renders commands only (shells without workspace context). */
  scope: PaletteSearchScope | null;
  /** Runs when a server hit activates; the panel is already closed. */
  onSelectSearchResult: (hit: SearchResultHit) => void;
}

export function TopBarSearch({ scope, onSelectSearchResult }: TopBarSearchProps): React.ReactElement {
  const [term, setTerm] = React.useState("");
  const [open, setOpen] = React.useState(false);
  const modalOpen = useCommandPalette((state) => state.open);
  const openPalette = useCommandPalette((state) => state.openPalette);
  const openPage = useCommandPalette((state) => state.openPage);
  const containerRef = React.useRef<HTMLDivElement>(null);

  const close = React.useCallback(() => {
    setOpen(false);
    setTerm("");
  }, []);

  React.useEffect(() => {
    if (modalOpen) close();
  }, [modalOpen, close]);

  React.useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      // Mounted while the panel is open, so the container always exists.
      if (!containerRef.current!.contains(event.target as Node)) close();
    };
    globalThis.document.addEventListener("pointerdown", onPointerDown);
    return () => {
      globalThis.document.removeEventListener("pointerdown", onPointerDown);
    };
  }, [open, close]);

  const commands = React.useMemo(() => filterCommands(listCommands(), term), [term]);
  const debounced = useDebouncedValue(term, PALETTE_SEARCH_DEBOUNCE_MS);
  const canSearch = scope !== null && debounced.trim().length > 0;
  // Clearing hides the section at once instead of trailing the debounce.
  const showResults = canSearch && term.trim().length > 0;

  const search = useQuery({
    ...paletteSearchQueryOptions(getAppClient(), scope?.workspaceSlug ?? "", {
      query: debounced,
      projectId: scope?.projectId ?? null,
      // The inline panel has no scope toggle: project-scoped in a project.
      workspaceSearch: false,
    }),
    enabled: canSearch,
  });
  const groups = React.useMemo<SearchResultGroup[]>(
    () => (showResults && search.data ? groupSearchResults(search.data) : []),
    [showResults, search.data]
  );
  const fetching = showResults && search.isFetching;
  const showNoResults = showResults && !search.isFetching && countSearchHits(groups) === 0;

  const rows = React.useMemo<PaletteRow[]>(
    () => [
      ...commands.map((command) => ({ kind: "command" as const, command })),
      ...(showNoResults
        ? [{ kind: "clear-search" as const }]
        : groups.flatMap((group) => group.hits.map((hit) => ({ kind: "hit" as const, hit })))),
    ],
    [commands, groups, showNoResults]
  );

  const activate = React.useCallback(
    (index: number) => {
      // The hook clamps into range, so the row always exists.
      const row = rows[index]!;
      if (row.kind === "clear-search") {
        setTerm("");
      } else if (row.kind === "hit") {
        close();
        onSelectSearchResult(row.hit);
      } else if (row.command.page !== undefined) {
        // Sub-pages live in the modal: hand the panel over to it.
        close();
        openPalette();
        openPage(row.command.page);
      } else {
        close();
        void row.command.run();
      }
    },
    [rows, close, onSelectSearchResult, openPalette, openPage]
  );

  const { activeIndex, setActiveIndex, onKeyDown } = usePaletteKeys({
    count: rows.length,
    query: term,
    onActivate: activate,
    // One press clears and closes together (SHELL-081/083).
    onClearQuery: close,
    onDismiss: close,
  });

  const grouped = React.useMemo(() => groupCommands(commands), [commands]);
  const renderGroups = React.useMemo(() => {
    let next = commands.length;
    return groups.map((group) => {
      const start = next;
      next += group.hits.length;
      return { group, entries: group.hits.map((hit, offset) => ({ hit, index: start + offset })) };
    });
  }, [commands.length, groups]);

  return (
    <div ref={containerRef} className="relative w-full max-w-md">
      <input
        type="text"
        aria-label="Search commands"
        placeholder={TOPBAR_SEARCH_PLACEHOLDER}
        value={term}
        onChange={(event) => setTerm(event.target.value)}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        className="text-body h-(--control-height) w-full rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) text-(--text) placeholder:text-(--text-muted) focus:border-(--accent) focus:outline-none"
      />
      {open ? (
        <div
          role="listbox"
          aria-label="Top bar search results"
          className="shadow-dialog absolute top-full right-0 left-0 z-50 mt-(--space-2) max-h-80 overflow-auto rounded-(--radius-dialog) border border-(--border) bg-(--bg)"
        >
          {commands.length === 0 && !showResults ? (
            <p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">No matching commands.</p>
          ) : (
            grouped.map((group) => (
              <div key={group.section}>
                <p className="text-caption px-(--space-2) pt-(--space-3) pb-(--space-1) text-(--text-muted)">
                  {group.section}
                </p>
                <ul>
                  {group.entries.map(({ command, index }) => (
                    <PaletteOption
                      key={command.id}
                      active={index === activeIndex}
                      title={command.title}
                      hint={command.hint}
                      shortcut={command.shortcut}
                      onHover={() => setActiveIndex(index)}
                      onSelect={() => activate(index)}
                    />
                  ))}
                </ul>
              </div>
            ))
          )}
          {showResults ? (
            <section aria-busy={fetching || undefined} aria-label="Server results">
              <p
                className={
                  fetching
                    ? "text-caption animate-pulse px-(--space-2) pt-(--space-3) pb-(--space-1) text-(--text-muted)"
                    : "text-caption px-(--space-2) pt-(--space-3) pb-(--space-1) text-(--text-muted)"
                }
              >
                Search results for &ldquo;{debounced.trim()}&rdquo;
              </p>
              {fetching && countSearchHits(groups) === 0 ? (
                <p className="px-(--space-2) py-(--space-4)">
                  <Spinner label="Searching" />
                </p>
              ) : null}
              {renderGroups.map(({ group, entries }) => (
                <div key={group.entity}>
                  <p className="text-caption px-(--space-2) pt-(--space-2) pb-(--space-1) text-(--text-muted)">
                    {group.heading}
                  </p>
                  <ul>
                    {entries.map(({ hit, index }) => (
                      <PaletteOption
                        key={hit.key}
                        active={index === activeIndex}
                        title={hit.title}
                        hint={hit.path}
                        onHover={() => setActiveIndex(index)}
                        onSelect={() => activate(index)}
                      />
                    ))}
                  </ul>
                </div>
              ))}
              {showNoResults ? (
                <ul>
                  <PaletteOption
                    active={commands.length === activeIndex}
                    title="No results found"
                    hint="Clear search"
                    onHover={() => setActiveIndex(commands.length)}
                    onSelect={() => activate(commands.length)}
                  />
                </ul>
              ) : null}
            </section>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
