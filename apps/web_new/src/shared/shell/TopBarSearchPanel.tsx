// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Top-bar search rows (shell spec, SHELL-081/084/085/093). The lazy half
// of TopBarSearch: command filtering, the debounced workspace search
// query and the grouped option rows. Mounts inside the eager listbox
// shell on first panel open (NEWFRONT-523, size gate) and reports its
// row count and activation back through onPanelApi, so the eager input
// drives arrows and Enter exactly as before. Clearing hides the server
// section at once instead of trailing the debounce.

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
import { groupCommands, PaletteOption, type PaletteRow } from "./paletteRows.js";

/** The row driver the panel reports to the eager input. */
export interface TopBarSearchPanelApi {
  /** Rows available to the highlight; zero disables move and activate. */
  count: number;
  /** Activate the row at the clamped index. */
  activate: (index: number) => void;
}

export interface TopBarSearchPanelProps {
  /** Current filter text, owned by the eager input. */
  term: string;
  /** Null renders commands only (shells without workspace context). */
  scope: PaletteSearchScope | null;
  /** Clamped highlight index from the eager keyboard flow. */
  activeIndex: number;
  /** Move the highlight directly (pointer hover). */
  onActiveIndexChange: (index: number) => void;
  /** Write the filter text (the no-hits row clears it). */
  onTermChange: (term: string) => void;
  /** Close the panel and reset the term. */
  onClose: () => void;
  /** Runs when a server hit activates; the panel is already closed. */
  onSelectSearchResult: (hit: SearchResultHit) => void;
  /** Receives the row driver whenever the rows change. */
  onPanelApi: (api: TopBarSearchPanelApi) => void;
}

export function TopBarSearchPanel({
  term,
  scope,
  activeIndex,
  onActiveIndexChange,
  onTermChange,
  onClose,
  onSelectSearchResult,
  onPanelApi,
}: TopBarSearchPanelProps): React.ReactElement {
  const openPalette = useCommandPalette((state) => state.openPalette);
  const openPage = useCommandPalette((state) => state.openPage);

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
        onTermChange("");
      } else if (row.kind === "hit") {
        onClose();
        onSelectSearchResult(row.hit);
      } else if (row.command.page !== undefined) {
        // Sub-pages live in the modal: hand the panel over to it.
        onClose();
        openPalette();
        openPage(row.command.page);
      } else {
        onClose();
        void row.command.run();
      }
    },
    [rows, onClose, onSelectSearchResult, onTermChange, openPalette, openPage]
  );

  React.useEffect(() => {
    onPanelApi({ count: rows.length, activate });
  }, [rows.length, activate, onPanelApi]);

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
    <>
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
                  onHover={() => onActiveIndexChange(index)}
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
                    onHover={() => onActiveIndexChange(index)}
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
                onHover={() => onActiveIndexChange(commands.length)}
                onSelect={() => activate(commands.length)}
              />
            </ul>
          ) : null}
        </section>
      ) : null}
    </>
  );
}
