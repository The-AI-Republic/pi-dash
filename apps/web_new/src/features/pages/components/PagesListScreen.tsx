// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project pages list (parity/specs/pages.md §3.1–§3.6, §3.8–§3.10,
// PAGE-001–021, and §12.2 PAGE-084–090). Reads the project row for the
// page_view gate and the one collection for the top-level rows; search,
// sort and filters are client state and reset when the project changes.
// Switching tabs never refetches. With nested pages the rows render as a
// tree that reads children per expanded parent, while search, filters and
// favourites search the whole visible forest and show each hit's parent
// path. The header carries the two creation entries, rows carry the
// favourite star and the shared row menu, and every write re-reads the one
// collection.

import { Spinner } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAdmin, isProjectAuthorized } from "../../projects/index.js";
import { usePickerProjectMembers, usePickerWorkspaceMembers } from "../../../shared/pickers/reference.js";
import { selectMemberOptions } from "../../../shared/pickers/members.js";
import { pagesListQueryOptions } from "../api/queries.js";
import { useDuplicatePage, useQuickCreatePage } from "../api/mutations.js";
import { useAddSubPage, usePageReparent, usePageSubtreeArchive } from "../api/nestedMutations.js";
import { pageForestQueryOptions } from "../api/treeQueries.js";
import {
  EMPTY_PAGE_FILTERS,
  DEFAULT_PAGE_SORT,
  hasPageFilters,
  pageInTab,
  pageCreatorName,
  pageSessionUserId,
  pagesFeatureHref,
  sortPageRows,
  visiblePageRows,
  type PageCreatorOption,
  type PageFilters,
  type PageListTab,
  type PageSort,
} from "../lib/pageList.js";
import { pageDetailHref } from "../lib/pageList.js";
import { quickCreateAccess } from "../lib/pageCreate.js";
import { PAGES_CRUMB, pagesTitle, resolvePagesBody } from "../lib/pageScreen.js";
import { copyPageLink, pageLinkUrl } from "../lib/share.js";
import { PagesEmptyState } from "./PagesEmptyState.js";
import { PagesFeatureGate } from "./PagesFeatureGate.js";
import { PagesFilterChips } from "./PagesFilterChips.js";
import { PagesFilterMenu } from "./PagesFilterMenu.js";
import { PagesListSkeleton } from "./PagesListSkeleton.js";
import { PagesListTabs } from "./PagesListTabs.js";
import { PagesNoResults } from "./PagesNoResults.js";
import { PagesSearch } from "./PagesSearch.js";
import { PagesSortMenu } from "./PagesSortMenu.js";
import { PageCreateDialogEntry, PagesQuickCreate } from "./PagesCreateEntry.js";
import { PageRowConnected } from "./PageRowConnected.js";
import { PageNestedMoveDialog } from "./PageNestedMoveDialog.js";
import { PageTree } from "./PageTree.js";
import { pageDiscoveryRows, pageTreeRoots, subPageAccess, type PageNestedRow } from "../lib/pageNested.js";
import { edition, isPageMoveEnabled } from "../../../core/edition/index.js";
import { registerCommands, registerPalettePages } from "../../../shared/commands/index.js";
import { buildPageJumpCommand } from "../palette/pageJumpCommands.js";
import { projectPagesJumpPage } from "../palette/pageJumpPage.js";
import type { PageRow as PageRowData } from "@pidash/api-client";

export interface PagesListScreenProps {
  workspaceSlug: string;
  projectId: string;
  tab: PageListTab;
}

/** Reset every client dimension when the project changes (PAGE-021). */
interface ClientState {
  projectId: string;
  query: string;
  sort: PageSort;
  filters: PageFilters;
}

function initialClientState(projectId: string): ClientState {
  return { projectId, query: "", sort: DEFAULT_PAGE_SORT, filters: EMPTY_PAGE_FILTERS };
}

export function PagesListScreen({ workspaceSlug, projectId, tab }: PagesListScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const me = useQuery(meQueryOptions(getAppClient()));
  const gateOff = project.data?.page_view === false;
  const pages = useQuery({
    ...pagesListQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: project.isSuccess && !gateOff,
  });
  const projectMembers = usePickerProjectMembers(workspaceSlug, projectId, project.data?.page_view !== false);
  const directory = usePickerWorkspaceMembers(workspaceSlug, !gateOff);

  const [state, setState] = React.useState<ClientState>(() => initialClientState(projectId));
  const client = state.projectId === projectId ? state : initialClientState(projectId);

  const update = (patch: Partial<Omit<ClientState, "projectId">>): void => {
    setState((current) => ({
      ...(current.projectId === projectId ? current : initialClientState(projectId)),
      ...patch,
      projectId,
    }));
  };

  const duplicate = useDuplicatePage(workspaceSlug, projectId);
  const goToPage = (pageId: string): void => {
    void navigate({ href: pageDetailHref(workspaceSlug, projectId, pageId) });
  };

  // The pages palette command is a navigation entry that exists on the
  // list route only while the project's pages switch is on (PAGE-090).
  const projectName = project.data?.name ?? projectId;
  React.useEffect(() => {
    const command = buildPageJumpCommand({
      projectName,
      pagesEnabled: project.data?.page_view !== false,
    });
    return command === null ? undefined : registerCommands([command]);
  }, [projectName, project.data?.page_view]);
  React.useEffect(
    () =>
      registerPalettePages([
        projectPagesJumpPage({
          workspaceSlug,
          projectId,
          onPick: (pageId) => {
            void navigate({ href: pageDetailHref(workspaceSlug, projectId, pageId) });
          },
        }),
      ]),
    [workspaceSlug, projectId, navigate]
  );

  const [now] = React.useState(() => new Date());
  const rows = React.useMemo(() => pages.data ?? [], [pages.data]);
  const tabRows = React.useMemo(() => rows.filter((row) => pageInTab(row, tab)), [rows, tab]);
  const searchActiveEarly = client.query.trim() !== "";
  const filtersActiveEarly = hasPageFilters(client.filters);
  const discoveryActive = searchActiveEarly || filtersActiveEarly;
  // Discovery (search/filters/favourites) spans every depth, so it reads
  // the whole visible forest; the plain list stays on the one top-level
  // collection and the tree reads children lazily per expanded parent.
  const forest = useQuery({
    ...pageForestQueryOptions(getAppClient(), workspaceSlug, projectId, discoveryActive),
    enabled: discoveryActive && project.isSuccess && !gateOff,
  });
  const discoveryPool = discoveryActive ? (forest.data?.rows ?? rows) : rows;
  const activeRows = discoveryActive ? discoveryPool : rows;
  const visible = React.useMemo(
    () => visiblePageRows(activeRows, tab, client.sort, client.query, client.filters, now),
    [activeRows, tab, client.sort, client.query, client.filters, now]
  );
  const tabbedPool = React.useMemo(
    () => (discoveryActive ? activeRows.filter((row) => pageInTab(row, tab)) : tabRows),
    [discoveryActive, activeRows, tab, tabRows]
  );
  const treeRoots = React.useMemo(
    () => pageTreeRoots(activeRows, tabbedPool, (pool) => sortPageRows(pool, client.sort)),
    [activeRows, tabbedPool, client.sort]
  );
  const discoveryRows = React.useMemo(
    () =>
      pageDiscoveryRows(activeRows, searchActiveEarly, filtersActiveEarly, (pool) =>
        visiblePageRows(pool, tab, client.sort, client.query, client.filters, now)
      ),
    [activeRows, searchActiveEarly, filtersActiveEarly, tab, client.sort, client.query, client.filters, now]
  );
  const currentUserId = pageSessionUserId(me.data);
  const creators = React.useMemo<PageCreatorOption[]>(
    () =>
      projectMembers.data === undefined || directory.data === undefined
        ? []
        : selectMemberOptions({
            projectMembers: projectMembers.data,
            directory: directory.data,
            meId: currentUserId,
            selectedIds: [],
          }).map((option) => ({ id: option.userId, name: option.searchText, avatarUrl: option.avatarUrl })),
    [currentUserId, directory.data, projectMembers.data]
  );
  const creatorName = pageCreatorName(creators, client.filters.creator);
  const role = roles.data?.[projectId] ?? null;
  const viewerIsAdmin = isProjectAdmin(role);
  const canCreate = isProjectAuthorized(role);
  const canMovePages = isPageMoveEnabled(edition);
  const canStar = isProjectAuthorized(role);
  const canManageFeatures = isProjectAdmin(role);
  const refreshing = pages.isFetching && !pages.isPending && rows.length > 0;
  const searchActive = client.query.trim() !== "";
  const filtersActive = hasPageFilters(client.filters);
  const quickCreate = useQuickCreatePage(workspaceSlug, projectId, quickCreateAccess(tab));
  const createQuickPage = (): void => {
    quickCreate.create((page) => goToPage(page.id));
  };
  const addSubPage = useAddSubPage(workspaceSlug, projectId);
  const createSubPage = (parent: PageRowData): void => {
    addSubPage.mutate(
      { parentId: parent.id, access: subPageAccess(parent) },
      { onSuccess: (created) => goToPage(created.id) }
    );
  };
  const subtreeArchive = usePageSubtreeArchive(workspaceSlug, projectId, discoveryPool);
  const archiveSubtree = (page: PageRowData, archived: boolean): void => {
    subtreeArchive.mutate({ pageId: page.id, row: page, archived });
  };
  const [moveTarget, setMoveTarget] = React.useState<PageRowData | null>(null);
  const reparent = usePageReparent(workspaceSlug, projectId);
  // The picker must offer every visible page, including children that the
  // lazy tree has never expanded, so it reads the forest while it is open.
  const movePoolQuery = useQuery({
    ...pageForestQueryOptions(getAppClient(), workspaceSlug, projectId, moveTarget !== null),
    enabled: moveTarget !== null && project.isSuccess && !gateOff,
  });
  // A row from the lazy tree need not be in the top-level collection.
  const movePool = React.useMemo(() => {
    const forestRows = movePoolQuery.data?.rows ?? discoveryPool;
    if (moveTarget === null || forestRows.some((row) => row.id === moveTarget.id)) return forestRows;
    return [moveTarget, ...forestRows];
  }, [moveTarget, movePoolQuery.data, discoveryPool]);
  const openInNewTab = (pageId: string): void => {
    globalThis.window.open(pageLinkUrl(workspaceSlug, projectId, pageId), "_blank", "noopener");
  };

  useDocumentHead({
    title: project.data === undefined ? PAGES_CRUMB : pagesTitle(project.data.name),
    robots: "noindex, nofollow",
  });

  const body = resolvePagesBody({
    projectPending: project.isPending && project.data === undefined,
    projectError: project.isError && project.data === undefined,
    gateOff,
    pagesPending: pages.isPending,
    pagesError: pages.isError,
    pageCount: rows.length,
    tabCount: tabRows.length,
    resultCount: visible.length,
    searchActive,
    filtersActive,
  });

  const renderFlatRow = (entry: PageNestedRow): React.ReactElement => {
    const page = entry.page;
    const owner = creators.find((creator) => creator.id === page.owned_by) ?? null;
    return (
      <PageRowConnected
        key={page.id}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        page={page}
        owner={owner}
        showStar={canStar}
        canDuplicate={canCreate}
        canManage={viewerIsAdmin || (currentUserId !== null && page.owned_by === currentUserId)}
        canMove={canMovePages}
        parentPath={entry.parentPath}
        rehomedCount={page.sub_pages_count ?? 0}
        onToggleArchive={(archived) => {
          archiveSubtree(page, archived);
        }}
        onAddSubPage={canCreate ? () => createSubPage(page) : undefined}
        onOpenInNewTab={() => {
          openInNewTab(page.id);
        }}
        onCopyLink={() => {
          void copyPageLink(workspaceSlug, projectId, page.id);
        }}
        onDuplicate={() => {
          duplicate.mutate({ pageId: page.id });
        }}
      />
    );
  };

  return (
    <section
      aria-label={PAGES_CRUMB}
      data-testid="pages-list-screen"
      className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
    >
      {body === "gate" ? (
        <PagesFeatureGate
          isAdmin={canManageFeatures}
          onManageFeatures={() => {
            void navigate({ to: pagesFeatureHref(workspaceSlug, projectId) });
          }}
        />
      ) : (
        <>
          {body !== "skeleton" && canCreate ? (
            <div className="flex items-center justify-end">
              <PagesQuickCreate pending={quickCreate.pending} onPress={createQuickPage} />
            </div>
          ) : null}
          <PageCreateDialogEntry
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            tab={tab}
            allowed={canCreate}
            onOpenPage={goToPage}
          />
          {body === "skeleton" ? <PagesListSkeleton /> : null}
          {body !== "skeleton" && rows.length > 0 ? (
            <>
              <div className="flex flex-wrap items-center gap-(--space-2)">
                <PagesListTabs workspaceSlug={workspaceSlug} projectId={projectId} activeTab={tab} />
                <span className="flex-1" aria-hidden="true" />
                <PagesSearch
                  key={`search-${projectId}`}
                  query={client.query}
                  onQueryChange={(query) => update({ query })}
                />
                <PagesSortMenu sort={client.sort} onSortChange={(sort) => update({ sort })} />
                <PagesFilterMenu
                  key={`filters-${projectId}`}
                  filters={client.filters}
                  creators={creators}
                  currentUserId={currentUserId}
                  onFiltersChange={(filters) => update({ filters })}
                />
                {refreshing ? (
                  <span
                    data-testid="pages-refresh-status"
                    className="text-caption inline-flex items-center gap-(--space-1) text-(--text-muted)"
                  >
                    <Spinner size="small" label="Refreshing pages" />
                  </span>
                ) : null}
              </div>
              <PagesFilterChips
                filters={client.filters}
                creatorName={creatorName}
                onFiltersChange={(filters) => update({ filters })}
              />
            </>
          ) : null}
          {body === "zero" ? (
            <PagesEmptyState
              archived={tab === "archived"}
              canCreate={canCreate}
              readFailed={pages.isError}
              onCreate={createQuickPage}
            />
          ) : null}
          {body === "no-results" ? (
            <PagesNoResults
              search={searchActive}
              filters={filtersActive}
              onClearSearch={() => update({ query: "" })}
              onClearFilters={() => update({ filters: EMPTY_PAGE_FILTERS })}
            />
          ) : null}
          {body === "list" ? (
            <div data-testid="pages-list" className="flex min-h-0 flex-1 flex-col">
              {discoveryRows === null ? (
                <PageTree
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  rows={treeRoots}
                  pool={rows}
                  showStar={canStar}
                  canDuplicate={canCreate}
                  currentUserId={currentUserId}
                  viewerIsAdmin={viewerIsAdmin}
                  canMove={canCreate}
                  canMoveProject={canMovePages}
                  owners={creators}
                  onCreateSubPage={createSubPage}
                  onOpenInNewTab={(page) => {
                    openInNewTab(page.id);
                  }}
                  onCopyLink={(page) => {
                    void copyPageLink(workspaceSlug, projectId, page.id);
                  }}
                  onDuplicate={(page) => {
                    duplicate.mutate({ pageId: page.id });
                  }}
                  onMovePage={(page, targetId) => {
                    reparent.mutate({ pageId: page.id, parentId: targetId });
                  }}
                  onToggleArchive={archiveSubtree}
                  onRequestMove={(page) => {
                    setMoveTarget(page);
                  }}
                />
              ) : (
                discoveryRows.map((entry: PageNestedRow) => renderFlatRow(entry))
              )}
            </div>
          ) : null}
          {moveTarget === null ? null : (
            <PageNestedMoveDialog
              open
              onOpenChange={(next) => {
                if (!next) setMoveTarget(null);
              }}
              page={moveTarget}
              pool={movePool}
              pending={reparent.isPending}
              loading={movePoolQuery.isPending}
              onConfirm={(targetId) => {
                reparent.mutate(
                  { pageId: moveTarget.id, parentId: targetId },
                  {
                    onSuccess: () => {
                      setMoveTarget(null);
                    },
                  }
                );
              }}
            />
          )}
        </>
      )}
    </section>
  );
}
