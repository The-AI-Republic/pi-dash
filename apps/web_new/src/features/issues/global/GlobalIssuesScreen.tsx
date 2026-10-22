// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues view (parity/specs/issues.md §18, ISS-223). One
// flat list across every project the caller can see, paged with the
// server cursor so the whole set is reachable. The route parses the
// URL's query parameters into route filters and hands them over; this
// build deliberately does not apply them (the oracle pins the
// unfiltered result), so the screen shows the same rows either way.

import { useSuspenseInfiniteQuery, useSuspenseQuery } from "@tanstack/react-query";
import type { GlobalIssueRow } from "@pidash/api-client";
import { Link, useParams, useSearch } from "@tanstack/react-router";
import { Button, VirtualList } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { hasRouteFilters, parseRouteFilters } from "./routeFilters.js";
import { flattenGlobalPages, globalIssuesQueryOptions, globalTotalCount } from "./queries.js";

/** Stable row height for the virtual list. */
export const GLOBAL_ROW_HEIGHT = 40;

/** The control that appends the next cursor page. */
export const GLOBAL_LOAD_MORE_LABEL = "Load more issues";

/** List sentinel that asks for the next page once it scrolls into view. */
function useLoadMoreSentinel(onReach: () => void, enabled: boolean): React.RefObject<HTMLDivElement | null> {
  const ref = React.useRef<HTMLDivElement | null>(null);
  React.useEffect(() => {
    if (!enabled || ref.current === null) return;
    const node = ref.current;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) onReach();
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [enabled, onReach]);
  return ref;
}

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
      className="text-body flex h-full items-center gap-(--space-2) border-b border-(--border) px-(--space-3) text-(--text) hover:bg-(--subtle)"
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
  const issues = useSuspenseInfiniteQuery(globalIssuesQueryOptions(client, workspaceSlug));
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const identifiers = React.useMemo(
    () => new Map(projects.map((project) => [project.id, project.identifier])),
    [projects]
  );
  const rows = React.useMemo(() => flattenGlobalPages(issues.data), [issues.data]);
  const total = globalTotalCount(issues.data);
  // A sentinel callback and a click can both land before the render that
  // reports the fetch state, so the latch is synchronous and the render
  // flag only keeps the control's busy state in step.
  const inFlight = React.useRef(false);
  const loadMore = React.useCallback(() => {
    if (!issues.hasNextPage || inFlight.current || issues.isFetchingNextPage) return;
    inFlight.current = true;
    void issues.fetchNextPage().finally(() => {
      inFlight.current = false;
    });
  }, [issues.hasNextPage, issues.isFetchingNextPage, issues.fetchNextPage]);
  const sentinel = useLoadMoreSentinel(loadMore, issues.hasNextPage);
  const filtersOn = hasRouteFilters(parseRouteFilters(routeFilters));
  if (rows.length === 0) {
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
        <span className="text-meta text-(--text-muted)">{total}</span>
      </header>
      <VirtualList
        label="All issues"
        items={rows}
        estimateSize={GLOBAL_ROW_HEIGHT}
        getKey={(row) => row.id}
        overscan={6}
        className="min-h-0 flex-1"
        renderRow={(row) => (
          <GlobalRow row={row} identifier={identifiers.get(row.project_id)} workspaceSlug={workspaceSlug} />
        )}
        footer={
          issues.hasNextPage ? (
            <div ref={sentinel} className="flex justify-center p-(--space-3)">
              <Button size="small" variant="secondary" loading={issues.isFetchingNextPage} onClick={loadMore}>
                {GLOBAL_LOAD_MORE_LABEL}
              </Button>
            </div>
          ) : null
        }
      />
    </section>
  );
}
