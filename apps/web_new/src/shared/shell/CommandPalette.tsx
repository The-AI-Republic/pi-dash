// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Centered command-palette modal (Architecture > shared/shell; shell spec
// §8, SHELL-080/082-085/090-093). Reads the shared command registry,
// filters by title plus keywords, and pairs the commands with debounced
// server search. Sub-pages (pickers, option lists) render through the
// generic page view. Entity screens register contextual commands (marked
// with their family and subject); the modal names the subject in a chip
// above the input, hides the chip plus the group on dismiss, and resets
// the dismissal on every open. DOM contract for the area-gate driver
// (NEWFRONT-295): role=dialog; the input placeholder is
// PALETTE_ROOT_PLACEHOLDER on root and the page's own prompt on a
// sub-page; one role=listbox ("Commands" on root, "Options" on a page);
// rows are role=option with a single aria-selected=true; group and
// result headings are plain text; the results heading pulses (aria-busy
// plus animate-pulse) mid-flight; the footer scope control is role=switch
// named "Workspace level"; the no-hits row is the option "No results
// found"; the context chip names "indicator · subject" with a clear
// button named "Clear context (Backspace)".

import { useQuery } from "@tanstack/react-query";
import { Dialog, Input, Spinner } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../core/api/index.js";
import {
  PALETTE_SEARCH_DEBOUNCE_MS,
  countSearchHits,
  filterCommands,
  findPalettePage,
  groupSearchResults,
  listCommands,
  paletteSearchQueryOptions,
  useCommandPalette,
  useDebouncedValue,
  type PalettePage,
  type PaletteSearchScope,
  type SearchResultGroup,
  type SearchResultHit,
} from "../commands/index.js";
import { ContextChip } from "./contextual/chip.js";
import { usePaletteKeys } from "./paletteKeys.js";
import { groupCommands, PaletteOption, type PaletteRow } from "./paletteRows.js";

/** Root input prompt; sub-pages show their own placeholder (SHELL-082). */
export const PALETTE_ROOT_PLACEHOLDER = "Type a command or search";

export interface CommandPaletteProps {
  /** Null renders commands only (shells without workspace context). */
  scope: PaletteSearchScope | null;
  /** Runs when a server hit activates; the palette is already closed. */
  onSelectSearchResult: (hit: SearchResultHit) => void;
}

export function CommandPalette({ scope, onSelectSearchResult }: CommandPaletteProps): React.ReactElement {
  const open = useCommandPalette((state) => state.open);
  const pageId = useCommandPalette((state) => state.page);
  const dismissed = useCommandPalette((state) => state.contextDismissed);
  const closePalette = useCommandPalette((state) => state.closePalette);
  const dismissContext = useCommandPalette((state) => state.dismissContext);
  const page = pageId === null ? undefined : findPalettePage(pageId);
  // The registry snapshots per open like the root view below.
  const subject = React.useMemo(
    () => listCommands().find((command) => command.context !== undefined)?.context,
    [open, pageId]
  );

  return (
    <Dialog title="Commands" open={open} onOpenChange={(next) => (next ? undefined : closePalette())}>
      {subject !== undefined && !dismissed ? (
        <ContextChip indicator={subject.indicator} subject={subject.subject} onClear={dismissContext} />
      ) : null}
      {page === undefined ? (
        <RootView scope={scope} onSelectSearchResult={onSelectSearchResult} />
      ) : (
        <PageView page={page} />
      )}
    </Dialog>
  );
}

function RootView({
  scope,
  onSelectSearchResult,
}: {
  scope: PaletteSearchScope | null;
  onSelectSearchResult: (hit: SearchResultHit) => void;
}): React.ReactElement {
  const open = useCommandPalette((state) => state.open);
  const query = useCommandPalette((state) => state.query);
  const workspaceSearch = useCommandPalette((state) => state.workspaceSearch);
  const dismissed = useCommandPalette((state) => state.contextDismissed);
  const setQuery = useCommandPalette((state) => state.setQuery);
  const closePalette = useCommandPalette((state) => state.closePalette);
  const openPage = useCommandPalette((state) => state.openPage);
  const setWorkspaceSearch = useCommandPalette((state) => state.setWorkspaceSearch);
  const dismissContext = useCommandPalette((state) => state.dismissContext);

  const commands = React.useMemo(() => {
    const visible = filterCommands(listCommands(), query);
    return dismissed ? visible.filter((command) => command.context === undefined) : visible;
  }, [query, open, dismissed]);
  const hasContext = React.useMemo(
    () => !dismissed && listCommands().some((command) => command.context !== undefined),
    // Snapshots with the command list above.
    [query, open, dismissed]
  );
  const debounced = useDebouncedValue(query, PALETTE_SEARCH_DEBOUNCE_MS);
  const canSearch = scope !== null && debounced.trim().length > 0;
  // Clearing hides the section at once instead of trailing the debounce.
  const showResults = canSearch && query.trim().length > 0;

  const search = useQuery({
    ...paletteSearchQueryOptions(getAppClient(), scope?.workspaceSlug ?? "", {
      query: debounced,
      projectId: scope?.projectId ?? null,
      workspaceSearch,
    }),
    enabled: canSearch,
  });
  const groups = React.useMemo<SearchResultGroup[]>(
    () => (showResults && search.data ? groupSearchResults(search.data) : []),
    [showResults, search.data]
  );
  const fetching = showResults && search.isFetching;
  // Failures land here too: with no data and zero groups the settled
  // search renders the no-hits row, never an error (SHELL-093).
  const showNoResults = showResults && !search.isFetching && countSearchHits(groups) === 0;
  const searchScope = showResults ? scope : null;

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
        setQuery("");
      } else if (row.kind === "hit") {
        closePalette();
        onSelectSearchResult(row.hit);
      } else if (row.command.page !== undefined) {
        openPage(row.command.page);
      } else {
        closePalette();
        void row.command.run();
      }
    },
    [rows, setQuery, closePalette, onSelectSearchResult, openPage]
  );

  const { activeIndex, setActiveIndex, onKeyDown } = usePaletteKeys({
    count: rows.length,
    query,
    onActivate: activate,
    onClearQuery: () => setQuery(""),
    onDismiss: closePalette,
    // Backspace on an empty root query dismisses the entity context.
    onStepBack: hasContext ? dismissContext : undefined,
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
  const toggleLabelId = React.useId();

  return (
    <div onKeyDown={onKeyDown}>
      <Input
        label="Filter commands"
        placeholder={PALETTE_ROOT_PLACEHOLDER}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        autoFocus
      />
      <div role="listbox" aria-label="Commands" className="mt-(--space-4) max-h-80 overflow-auto">
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
                    sequence={command.sequence}
                    onHover={() => setActiveIndex(index)}
                    onSelect={() => activate(index)}
                  />
                ))}
              </ul>
            </div>
          ))
        )}
        {searchScope ? (
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
            <div className="flex items-center justify-between gap-(--space-4) px-(--space-2) pt-(--space-4) pb-(--space-1)">
              <span id={toggleLabelId} className="text-caption text-(--text-muted)">
                Workspace level
              </span>
              <button
                type="button"
                role="switch"
                aria-labelledby={toggleLabelId}
                aria-checked={workspaceSearch}
                disabled={searchScope.projectId === null}
                onClick={() => setWorkspaceSearch(!workspaceSearch)}
                className="inline-flex h-5 w-9 shrink-0 items-center rounded-full border border-(--border) bg-(--subtle) px-0.5 disabled:cursor-not-allowed disabled:opacity-50 aria-checked:justify-end aria-checked:bg-(--accent)"
              >
                <span aria-hidden="true" className="size-3.5 rounded-full bg-(--bg)" />
              </button>
            </div>
          </section>
        ) : null}
      </div>
    </div>
  );
}

function PageView({ page }: { page: PalettePage }): React.ReactElement {
  const query = useCommandPalette((state) => state.query);
  const setQuery = useCommandPalette((state) => state.setQuery);
  const closePalette = useCommandPalette((state) => state.closePalette);
  const closePage = useCommandPalette((state) => state.closePage);

  const state = page.usePageState(query);
  const options = state.status === "ready" ? state.options : [];

  const activate = React.useCallback(
    (index: number) => {
      // The hook clamps into range, so the option always exists. The page
      // stays open: multi-change pickers dismiss themselves (SHELL-090).
      const option = options[index]!;
      void option.run();
    },
    [options]
  );

  const { activeIndex, setActiveIndex, onKeyDown } = usePaletteKeys({
    count: options.length,
    query,
    onActivate: activate,
    onClearQuery: () => setQuery(""),
    onDismiss: closePalette,
    onStepBack: closePage,
  });

  return (
    <div onKeyDown={onKeyDown}>
      <Input
        label="Filter options"
        placeholder={page.placeholder}
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        autoFocus
      />
      {state.status === "loading" ? (
        <p className="px-(--space-2) py-(--space-4)">
          <Spinner label="Loading options" />
        </p>
      ) : options.length === 0 ? (
        <p className="text-body px-(--space-2) py-(--space-4) text-(--text-muted)">{state.emptyText}</p>
      ) : (
        <div role="listbox" aria-label="Options" className="mt-(--space-4) max-h-80 overflow-auto">
          <ul>
            {options.map((option, index) => (
              <PaletteOption
                key={option.id}
                active={index === activeIndex}
                title={option.title}
                hint={option.hint}
                selected={option.selected}
                icon={option.icon}
                onHover={() => setActiveIndex(index)}
                onSelect={() => activate(index)}
              />
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
