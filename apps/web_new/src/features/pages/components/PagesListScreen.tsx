// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project pages list (parity/specs/pages.md §3.1–§3.6 and §3.8,
// PAGE-001–017). Reads the project row for the page_view gate and the one
// collection for the rows; search, sort and filters are client state and
// reset when the project changes. Switching tabs never refetches. The
// header carries the two creation entries, rows carry the favourite star
// and the shared row menu, and every write re-reads the one collection.

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
import {
  EMPTY_PAGE_FILTERS,
  DEFAULT_PAGE_SORT,
  hasPageFilters,
  pageInTab,
  pageCreatorName,
  pageSessionUserId,
  pagesFeatureHref,
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
import { edition, isPageMoveEnabled } from "../../../core/edition/index.js";

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

  const [now] = React.useState(() => new Date());
  const rows = React.useMemo(() => pages.data ?? [], [pages.data]);
  const tabRows = React.useMemo(() => rows.filter((row) => pageInTab(row, tab)), [rows, tab]);
  const visible = React.useMemo(
    () => visiblePageRows(rows, tab, client.sort, client.query, client.filters, now),
    [rows, tab, client.sort, client.query, client.filters, now]
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
  const searchActive = client.query.trim() !== "";
  const filtersActive = hasPageFilters(client.filters);
  const quickCreate = useQuickCreatePage(workspaceSlug, projectId, quickCreateAccess(tab));
  const createQuickPage = (): void => {
    quickCreate.create((page) => goToPage(page.id));
  };

  useDocumentHead({
    title: project.data === undefined ? PAGES_CRUMB : pagesTitle(project.data.name),
    robots: "noindex, nofollow",
  });

  const body = resolvePagesBody({
    projectPending: project.isPending,
    projectError: project.isError,
    gateOff,
    pagesPending: pages.isPending,
    pagesError: pages.isError,
    pageCount: rows.length,
    tabCount: tabRows.length,
    resultCount: visible.length,
    searchActive,
    filtersActive,
  });

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
          {body !== "skeleton" && body !== "error" && rows.length > 0 ? (
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
              {visible.map((page) => {
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
                    onOpenInNewTab={() => {
                      globalThis.window.open(pageLinkUrl(workspaceSlug, projectId, page.id), "_blank", "noopener");
                    }}
                    onCopyLink={() => {
                      void copyPageLink(workspaceSlug, projectId, page.id);
                    }}
                    onDuplicate={() => {
                      duplicate.mutate({ pageId: page.id });
                    }}
                  />
                );
              })}
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}
