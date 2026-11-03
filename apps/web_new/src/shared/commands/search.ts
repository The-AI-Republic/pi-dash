// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette server search (shell spec, SHELL-084/085/093). Both palette
// surfaces share this: the idle-pause debounce, the workspace search query
// (disabled on blank queries so zero network fires), and the grouping of
// raw entity rows into headed display groups. Failures are the caller's to
// render as empty — this module never throws past the query boundary.

import { keepPreviousData, queryOptions } from "@tanstack/react-query";
import { searchWorkspace, type ApiClient, type SearchEntity, type SearchResults } from "@pidash/api-client";
import * as React from "react";

import { workspaceScope } from "../../core/query/client.js";

/** Idle pause before a keystroke burst fires one search request (SHELL-084). */
export const PALETTE_SEARCH_DEBOUNCE_MS = 500;

/**
 * Where a palette surface searches. The workspace route passes both halves
 * down; null means a shell without workspace context, where the palette is
 * commands-only.
 */
export interface PaletteSearchScope {
  workspaceSlug: string;
  /** Project in context; null outside one (the scope toggle disables). */
  projectId: string | null;
}

/** Holds `value` back until it stays still for `delayMs`. */
export function useDebouncedValue(value: string, delayMs: number): string {
  const [debounced, setDebounced] = React.useState(value);
  React.useEffect(() => {
    const timer = globalThis.setTimeout(() => {
      setDebounced(value);
    }, delayMs);
    return () => {
      globalThis.clearTimeout(timer);
    };
  }, [value, delayMs]);
  return debounced;
}

export const paletteSearchKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "palette-search"] as const,
  search: (workspaceSlug: string, query: string, projectId: string | null, workspaceSearch: boolean) =>
    [...paletteSearchKeys.all(workspaceSlug), query, projectId, workspaceSearch] as const,
};

export interface PaletteSearchParams {
  /** Raw query text; trimmed before it hits the network. */
  query: string;
  /** Project in context, or null outside one. */
  projectId: string | null;
  /** True widens the search from the project to the workspace. */
  workspaceSearch: boolean;
}

/**
 * The palette's server search. Disabled while the trimmed query is blank
 * (SHELL-085: zero network calls) and fails fast without retries so a dead
 * backend renders as the no-hits row instead of hanging (SHELL-093). The
 * previous page of hits stays visible while a scope flip or a new burst
 * refetches (SHELL-084: the heading pulses over stale rows, not a gap).
 */
export function paletteSearchQueryOptions(client: ApiClient, workspaceSlug: string, params: PaletteSearchParams) {
  const search = params.query.trim();
  return queryOptions({
    queryKey: paletteSearchKeys.search(workspaceSlug, search, params.projectId, params.workspaceSearch),
    queryFn: () =>
      searchWorkspace(client, workspaceSlug, {
        search,
        workspaceSearch: params.workspaceSearch,
        ...(params.projectId === null ? {} : { projectId: params.projectId }),
      }),
    enabled: search.length > 0,
    retry: false,
    placeholderData: keepPreviousData,
  });
}

/** One display row inside a grouped server result. */
export interface SearchResultHit {
  /** Unique within the result set (`${entity}:${id}`). */
  key: string;
  entity: SearchEntity;
  /** Primary line: the entity name. */
  title: string;
  /** Context line: identifier, key or project, e.g. "ATL-12". */
  path: string;
  /** Raw contract row, for the route layer's navigation mapping. */
  raw: unknown;
}

/** One headed group of server hits ("Issues", "Projects", ...). */
export interface SearchResultGroup {
  entity: SearchEntity;
  heading: string;
  hits: SearchResultHit[];
}

const GROUP_ORDER: Array<{ entity: SearchEntity; heading: string }> = [
  { entity: "issue", heading: "Issues" },
  { entity: "project", heading: "Projects" },
  { entity: "cycle", heading: "Cycles" },
  { entity: "module", heading: "Modules" },
  { entity: "issue_view", heading: "Views" },
  { entity: "page", heading: "Pages" },
  { entity: "intake", heading: "Intake items" },
  { entity: "workspace", heading: "Workspaces" },
];

/**
 * Groups raw entity rows into headed display groups in palette order,
 * skipping entities with no rows. Only requested entities arrive (the
 * contract leaves the rest absent), and absent counts as empty.
 */
export function groupSearchResults(results: SearchResults): SearchResultGroup[] {
  const lists = results.results as Record<string, Array<Record<string, unknown>> | undefined>;
  const groups: SearchResultGroup[] = [];
  for (const { entity, heading } of GROUP_ORDER) {
    const rows = lists[entity] ?? [];
    if (rows.length === 0) continue;
    groups.push({
      entity,
      heading,
      hits: rows.map((row) => ({
        key: `${entity}:${String(row["id"])}`,
        entity,
        // Name and id are contract-required; the cast only widens the type.
        title: String(row["name"]),
        path: describeHit(entity, row),
        raw: row,
      })),
    });
  }
  return groups;
}

/** Total hits across every group; zero renders the no-hits row (SHELL-093). */
export function countSearchHits(groups: SearchResultGroup[]): number {
  return groups.reduce((total, group) => total + group.hits.length, 0);
}

function text(row: Record<string, unknown>, field: string): string {
  const value = row[field];
  return typeof value === "string" ? value : "";
}

function describeHit(entity: SearchEntity, row: Record<string, unknown>): string {
  switch (entity) {
    case "issue":
    case "intake": {
      const sequence = row["sequence_id"];
      const key = typeof sequence === "number" ? `${text(row, "project__identifier")}-${sequence}` : "";
      return key;
    }
    case "project":
      return text(row, "identifier");
    case "cycle":
    case "module":
    case "issue_view":
      return text(row, "project__identifier");
    case "page": {
      const identifiers = row["project_identifiers"];
      return Array.isArray(identifiers) ? identifiers.filter((entry) => typeof entry === "string").join(", ") : "";
    }
    case "workspace":
      return text(row, "slug");
  }
}
