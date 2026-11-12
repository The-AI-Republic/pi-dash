// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues view (parity/specs/issues.md §18, ISS-223). One
// flat list across every project the caller can see, paged with the
// server cursor so the whole set is reachable. The URL keeps its query
// parameters but this build deliberately does not apply them (the
// oracle pins the unfiltered result), so the screen shows the same rows
// either way, and the empty surface is the workspace card (ISS-073).
// Every row carries the project-style quick-action menu (ISS-065); the
// row's own state annotation answers the archive gate without a read,
// and the per-project role decides which write entries appear.
//
// The screen is an issues body, so it opens the same peek as the project
// layouts (parity/specs/issues.md §14, ISS-165/167): it takes the one
// shared address contract, and a row's title opens the panel on a plain
// click while the row keeps its full-page href for modified clicks. The
// workspace surface has no route project, so the peeked row's own
// project always rides the address.

import { useSuspenseInfiniteQuery, useSuspenseQuery } from "@tanstack/react-query";
import type { GlobalIssueRow, IssueListQuery } from "@pidash/api-client";
import { Link, useParams } from "@tanstack/react-router";
import { Button, VirtualList } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { GlobalIssuesEmptyState } from "../contexts/GlobalIssuesEmptyState.js";
import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { ListPageQuickActions } from "../menus/ListPageQuickActions.js";
import { RowContextMenu, RowMenuButton } from "../menus/RowQuickActions.js";
import { quickActionTarget, useQuickActions } from "../menus/useQuickActions.js";
import { usePeekAddress, type LayoutPeekProps, type PeekSelection } from "../peek/usePeekAddress.js";
import { flattenGlobalPages, globalIssuesQueryOptions, globalTotalCount } from "./queries.js";

// The peek is the whole detail composition; it loads on first open.
const LazyIssuePeek = React.lazy(() =>
  import("../peek/ResolvingIssuePeek.js").then((module) => ({ default: module.ResolvingIssuePeek }))
);

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

/**
 * The selection a row opens (ISS-165): its own id at the top of the
 * stack, read from the project that owns it rather than a route's.
 */
export function rowPeekSelection(row: Pick<GlobalIssueRow, "id" | "project_id">): PeekSelection {
  return { issueId: row.id, projectId: row.project_id, nestingLevel: 0 };
}

function GlobalRow({
  row,
  identifier,
  workspaceSlug,
  canWrite,
  onOpen,
}: {
  row: GlobalIssueRow;
  identifier: string | undefined;
  workspaceSlug: string;
  canWrite: boolean;
  /** Opens the peek on a plain click; the href stays for modified clicks. */
  onOpen: (row: GlobalIssueRow) => void;
}): React.ReactElement {
  const key = globalIssueKey(identifier, row);
  const menu = useQuickActions({
    workspaceSlug,
    target: {
      ...quickActionTarget(row, identifier ?? ""),
      stateGroup: row.state__group,
    },
    context: "global",
    canWrite,
  });
  // The menu trigger sits beside the link, not inside it: a button nested
  // in an anchor is invalid and swallows the link's own click.
  const body = (
    <div data-row-name={row.name} className="flex h-full items-center border-b border-(--border) hover:bg-(--subtle)">
      <Link
        to="/$ws/browse/$issueKey"
        params={{ ws: workspaceSlug, issueKey: key }}
        className="text-body flex h-full min-w-0 flex-1 items-center gap-(--space-2) px-(--space-3) text-(--text)"
        onClick={(event) => {
          // A modified click keeps the row's full-page address; a plain
          // one opens the peek, and the href stays for middle-click and
          // the keyboard's own activation path.
          if (event.metaKey || event.ctrlKey || event.shiftKey || event.button !== 0) return;
          event.preventDefault();
          onOpen(row);
        }}
      >
        <span className="text-meta w-20 shrink-0 text-(--text-muted)">{key}</span>
        <span className="min-w-0 flex-1 truncate">{row.name}</span>
      </Link>
      <RowMenuButton state={menu} issueKey={key} className="shrink-0 pr-(--space-2)" />
    </div>
  );
  return <RowContextMenu state={menu} trigger={body} />;
}

export interface GlobalIssuesScreenProps extends LayoutPeekProps {
  /** Scope the read: a saved view's expression, a default's static condition, an order. */
  query?: IssueListQuery | undefined;
  /** Hide the list's own header; an embedding screen carries its own chrome. */
  hideHeader?: boolean | undefined;
  /** Empty surface override; the workspace card renders by default. */
  emptyState?: React.ReactNode | undefined;
  /** List title for the header and the accessible names. */
  label?: string | undefined;
}

/**
 * Every visible issue in the workspace, one flat list. The mounted
 * route supplies both inputs: its workspace param and its raw search.
 * A saved-view detail embeds the same list scoped to the view.
 */
export function GlobalIssuesScreen({
  query = {},
  hideHeader = false,
  emptyState,
  label = "All issues",
  peekKey,
  peekProjectId,
  onPeekChange,
}: GlobalIssuesScreenProps = {}): React.ReactElement {
  const params = useParams({ strict: false }) as { ws?: string };
  const workspaceSlug = params.ws ?? "";
  const client = getAppClient();
  const issues = useSuspenseInfiniteQuery(globalIssuesQueryOptions(client, workspaceSlug, query));
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const identifiers = React.useMemo(
    () => new Map(projects.map((project) => [project.id, project.identifier])),
    [projects]
  );
  const rows = React.useMemo(() => flattenGlobalPages(issues.data), [issues.data]);
  // The visible rows span projects; a row is writable when the viewer
  // holds a writing role on its own project (archived projects read).
  const writableProjects = React.useMemo(
    () =>
      new Set(
        projects
          .filter((project) =>
            canEditInProject({ memberRole: project.member_role, archived: project.archived_at != null })
          )
          .map((project) => project.id)
      ),
    [projects]
  );
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
  // The workspace surface has no route project: every peeked row carries
  // its own, so the address always spells it (ISS-167).
  const peek = usePeekAddress({ projectId: "", peekKey, peekProjectId, onPeekChange });
  const openRow = React.useCallback(
    (row: GlobalIssueRow): void => {
      peek.open(rowPeekSelection(row));
    },
    [peek]
  );
  const peekedRow = peek.peeked === null ? undefined : rows.find((row) => row.id === peek.peeked);
  // A peeked item reads under its own project. The workspace route has no
  // path project to fall back to, so an address that names neither the
  // project nor a visible row has nothing to read and stays closed.
  const peekedProject = peek.peekProjectId ?? peekedRow?.project_id ?? null;
  const peekedIdentifier = peekedProject === null ? "" : (identifiers.get(peekedProject) ?? "");
  // The panel stands beside every body: a shared link with no rows yet
  // still has to open, and a nested sub-issue's parent keeps rendering
  // while the child holds the address.
  const panel =
    peek.peeked === null || peekedProject === null ? null : (
      <React.Suspense fallback={null}>
        <LazyIssuePeek
          workspaceSlug={workspaceSlug}
          projectId={peekedProject}
          projectIdentifier={peekedIdentifier}
          peeked={peek.peeked}
          peekProjectId={peek.peekProjectId}
          addressProjectId=""
          rows={rows}
          onPeekChange={peek.report}
          onClose={peek.close}
        />
      </React.Suspense>
    );
  if (rows.length === 0) {
    const surface =
      emptyState !== undefined ? (
        <>{emptyState}</>
      ) : (
        // The empty surface is the workspace card (ISS-073): the old list
        // shows the same two-tier card with or without query params,
        // because the read ignores them (ISS-223).
        <GlobalIssuesEmptyState workspaceSlug={workspaceSlug} projects={projects} />
      );
    return panel === null ? (
      surface
    ) : (
      <>
        {surface}
        {panel}
      </>
    );
  }
  return (
    <section aria-label={label} className="flex h-full min-h-0 flex-col">
      {panel}
      {hideHeader ? null : (
        <header className="flex items-center justify-between border-b border-(--border) px-(--space-3) py-(--space-2)">
          <h1 className="text-h3 text-(--text)">{label}</h1>
          <div className="ml-auto flex items-center gap-(--space-3)">
            <span className="text-meta text-(--text-muted)">{total}</span>
            <ListPageQuickActions />
          </div>
        </header>
      )}
      <VirtualList
        label={label}
        items={rows}
        estimateSize={GLOBAL_ROW_HEIGHT}
        getKey={(row) => row.id}
        overscan={6}
        className="min-h-0 flex-1"
        renderRow={(row) => (
          <GlobalRow
            row={row}
            identifier={identifiers.get(row.project_id)}
            workspaceSlug={workspaceSlug}
            canWrite={writableProjects.has(row.project_id)}
            onOpen={openRow}
          />
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
