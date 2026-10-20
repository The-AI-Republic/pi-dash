// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues view (parity/specs/issues.md §18, ISS-223). One
// flat, paged list across every project the caller can see. The route
// parses the URL's query parameters into route filters and hands them
// over; this build deliberately does not apply them (the oracle pins
// the unfiltered result), so the screen shows the same rows either way.

import { useSuspenseQuery } from "@tanstack/react-query";
import type { GlobalIssueRow } from "@pidash/api-client";
import { Link, useParams, useSearch } from "@tanstack/react-router";
import { VirtualList } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { hasRouteFilters, parseRouteFilters } from "./routeFilters.js";
import { globalIssuesQueryOptions } from "./queries.js";

/** Stable row height for the virtual list. */
export const GLOBAL_ROW_HEIGHT = 40;

/** The short key a row shows and links by. */
export function globalIssueKey(identifier: string | undefined, row: Pick<GlobalIssueRow, "sequence_id">): string {
  return `${identifier ?? "?"}-${row.sequence_id}`;
}

function GlobalRow({
  row,
  identifier,
  workspaceSlug,
}: {
  row: GlobalIssueRow;
  identifier: string | undefined;
  workspaceSlug: string;
}): React.ReactElement {
  const key = globalIssueKey(identifier, row);
  return (
    <Link
      to="/$ws/browse/$issueKey"
      params={{ ws: workspaceSlug, issueKey: key }}
      className="text-body flex h-full items-center gap-(--space-2) border-b border-(--border) px-(--space-3) text-(--text) hover:bg-(--surface-raised)"
    >
      <span className="text-meta w-20 shrink-0 text-(--text-muted)">{key}</span>
      <span className="min-w-0 truncate">{row.name}</span>
    </Link>
  );
}

/**
 * Every visible issue in the workspace, one flat list. The mounted
 * route supplies both inputs: its workspace param and its raw search.
 */
export function GlobalIssuesScreen(): React.ReactElement {
  const params = useParams({ strict: false }) as { ws?: string };
  const routeFilters = useSearch({ strict: false }) as Record<string, unknown>;
  const workspaceSlug = params.ws ?? "";
  const client = getAppClient();
  const { data: issues } = useSuspenseQuery(globalIssuesQueryOptions(client, workspaceSlug));
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const identifiers = React.useMemo(
    () => new Map(projects.map((project) => [project.id, project.identifier])),
    [projects]
  );
  const filtersOn = hasRouteFilters(parseRouteFilters(routeFilters));
  if (issues.results.length === 0) {
    return (
      <section aria-label="All issues" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">No issues yet</h2>
        <p className="text-body text-(--text-muted)">
          {filtersOn ? "No issues match these filters." : "Issues from every project will appear here."}
        </p>
      </section>
    );
  }
  return (
    <section aria-label="All issues" className="flex h-full min-h-0 flex-col">
      <header className="flex items-center justify-between border-b border-(--border) px-(--space-3) py-(--space-2)">
        <h1 className="text-h3 text-(--text)">All issues</h1>
        <span className="text-meta text-(--text-muted)">{issues.total_count}</span>
      </header>
      <VirtualList
        label="All issues"
        items={issues.results}
        estimateSize={GLOBAL_ROW_HEIGHT}
        getKey={(row) => row.id}
        overscan={6}
        className="min-h-0 flex-1"
        renderRow={(row) => (
          <GlobalRow row={row} identifier={identifiers.get(row.project_id)} workspaceSlug={workspaceSlug} />
        )}
      />
    </section>
  );
}
