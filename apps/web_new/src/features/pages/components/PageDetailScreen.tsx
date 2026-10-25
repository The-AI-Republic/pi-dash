// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages detail shell (parity/specs/pages.md §4, PAGE-022–025, 079–080).
// One record read by URL id with visit tracking plus the shared collection
// read that feeds the sibling switcher; the header cluster, the not-found
// panel, the centered record spinner, the title and body surfaces, the
// navigation pane and the version overlay. The editor, toolbar, pane tab
// bodies, header menu entries and collaboration client mount here in later
// slices; this shell owns their seams and the URL protocol they share.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { RichTextView } from "../../../shared/editor/RichTextView.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { isProjectAdmin } from "../../projects/index.js";
import { pageDetailQueryOptions, pageVersionQueryOptions, pageVersionsQueryOptions } from "../api/detailQueries.js";
import { usePageRename } from "../api/detailMutations.js";
import { pagesListQueryOptions } from "../api/queries.js";
import { isPageBodyReady, markPageBodyReady } from "../lib/pageBodyCache.js";
import {
  closeVersionSearch,
  historySearch,
  pageCapabilities,
  pageDocumentTitle,
  pageSiblingOptions,
  paneSearch,
  resolvePageDetailBody,
  siblingHref,
  versionSearch,
  type PageDetailSearch,
  type PagePaneTab,
} from "../lib/pageDetail.js";
import { copyPageLink } from "../lib/share.js";
import { usePageSyncStatus } from "../lib/pageSync.js";
import { PageBodySkeleton, PageDetailSpinner } from "./PageDetailLoading.js";
import { PageDetailPanel } from "./PageDetailPanel.js";
import { PageDetailHeader } from "./PageDetailHeader.js";
import { PageDetailTitle } from "./PageDetailTitle.js";
import { PageNavigationPane } from "./PageNavigationPane.js";
import { PageVersionOverlay } from "./PageVersionOverlay.js";
import { PAGE_UNTITLED } from "../lib/pageList.js";

export const PAGE_BODY_LABEL = "Page body";

export interface PageDetailScreenProps {
  workspaceSlug: string;
  projectId: string;
  pageId: string;
  search: PageDetailSearch;
  onSearchChange: (search: PageDetailSearch) => void;
}

export function PageDetailScreen({
  workspaceSlug,
  projectId,
  pageId,
  search,
  onSearchChange,
}: PageDetailScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const client = getAppClient();
  const project = useProjectRow(workspaceSlug, projectId);
  const roles = useProjectRoles(workspaceSlug);
  const me = useQuery(meQueryOptions(client));
  const pages = useQuery({
    ...pagesListQueryOptions(client, workspaceSlug, projectId),
    enabled: project.isSuccess && project.data.page_view !== false,
  });
  const record = useQuery(pageDetailQueryOptions(client, workspaceSlug, projectId, pageId));
  const paneOpen = search.paneTab !== undefined;
  const versions = useQuery({
    ...pageVersionsQueryOptions(client, workspaceSlug, projectId, pageId),
    enabled: paneOpen && search.paneTab === "info" && record.isSuccess,
  });
  const snapshot = useQuery({
    ...pageVersionQueryOptions(client, workspaceSlug, projectId, pageId, search.version ?? ""),
    enabled: search.version !== undefined && record.isSuccess,
  });

  const listRow = pages.data?.find((row) => row.id === pageId);
  const page = record.data;
  const role = roles.data?.[projectId] ?? null;
  const viewerId = me.data?.id ?? null;

  const capabilities = React.useMemo(() => {
    if (page === undefined) return null;
    return pageCapabilities({
      ownedBy: page.owned_by,
      access: page.access,
      archivedAt: page.archived_at,
      isLocked: page.is_locked,
      viewerId,
      projectRole: role,
    });
  }, [page, viewerId, role]);

  const body = resolvePageDetailBody({
    pending: record.isPending || me.isPending || roles.isPending,
    recordError: record.isError,
    canRead: capabilities?.canRead ?? true,
  });

  const name = page?.name ?? listRow?.name ?? "";
  useDocumentHead({
    title: pageDocumentTitle(name),
    robots: "noindex, nofollow",
  });

  const syncStatus = usePageSyncStatus(pageId);
  const rename = usePageRename(workspaceSlug, projectId, pageId);

  const siblings = React.useMemo(() => pageSiblingOptions(pages.data ?? []), [pages.data]);

  // The skeleton is a cold-load surface: one committed paint of the
  // placeholder, then the stored body, and never again for this page in
  // this session (PAGE-079). Marking before the paint would mean the
  // placeholder never actually reached the screen.
  const [bodyReady, setBodyReady] = React.useState(() => isPageBodyReady(pageId));
  React.useEffect(() => {
    setBodyReady(isPageBodyReady(pageId));
  }, [pageId]);
  React.useEffect(() => {
    if (body !== "editor" || bodyReady) return;
    const frame = requestAnimationFrame(() => {
      markPageBodyReady(pageId);
      setBodyReady(true);
    });
    return () => cancelAnimationFrame(frame);
  }, [body, bodyReady, pageId]);

  /** Every navigation from the detail rewrites the address's two keys. */
  const goTo = (href: string): void => {
    void navigate({ href });
  };

  const openPaneTab = (tab: PagePaneTab): void => {
    onSearchChange(paneSearch(search, tab));
  };

  const closePane = (): void => {
    onSearchChange(paneSearch(search, null));
  };

  const selectVersion = (versionId: string): void => {
    onSearchChange(versionSearch(versionId, search));
  };

  const closeVersion = (): void => {
    onSearchChange(closeVersionSearch(search));
  };

  if (body === "spinner") {
    return <PageDetailSpinner />;
  }

  if (body === "panel" || page === undefined || capabilities === null) {
    return <PageDetailPanel workspaceSlug={workspaceSlug} projectId={projectId} />;
  }

  const canManage = capabilities.canManage || isProjectAdmin(role);
  const projectName = project.data?.name ?? projectId;
  const pageTitle = page.name.trim() === "" ? PAGE_UNTITLED : page.name;

  return (
    <div data-testid="page-detail-screen" className="flex min-h-0 flex-1 flex-col">
      <PageDetailHeader
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName}
        projectLogo={(project.data?.logo_props ?? {}) as Record<string, unknown>}
        projectIdentifier={project.data?.identifier ?? ""}
        page={page}
        siblings={siblings}
        syncStatus={syncStatus}
        showStar={capabilities.standing !== "guest"}
        onSelectSibling={(id) => goTo(siblingHref(workspaceSlug, projectId, id))}
        onCopyLink={() => {
          void copyPageLink(workspaceSlug, projectId, page.id);
        }}
        onOpenHistory={() => onSearchChange(historySearch(search))}
      />
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 overflow-y-auto">
          <article aria-label={pageTitle} className="mx-auto w-full max-w-3xl px-(--space-8) py-(--space-6)">
            <PageDetailTitle
              pageId={page.id}
              name={page.name}
              editable={capabilities.canEdit}
              onRename={(next) => rename.mutate(next)}
            />
            {capabilities.isShelved || capabilities.isLocked ? (
              <p className="text-caption mb-(--space-4) text-(--text-muted)">
                {capabilities.isShelved ? "This page is archived and read-only." : "This page is locked and read-only."}
              </p>
            ) : null}
            <div id="page-content-container" className="relative w-full">
              <div className="frame-renderer w-full">
                <div className="editor-container">
                  {bodyReady ? (
                    <RichTextView html={page.description_html} label={PAGE_BODY_LABEL} />
                  ) : (
                    <PageBodySkeleton />
                  )}
                  {canManage ? <span data-testid="page-detail-manage-mount" hidden aria-hidden="true" /> : null}
                  <span data-testid="page-detail-action-mount" hidden aria-hidden="true" />
                </div>
              </div>
            </div>
          </article>
        </div>
        {paneOpen ? (
          <PageNavigationPane
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            pageId={page.id}
            search={search}
            versions={search.paneTab === "info" ? versions.data : undefined}
            onSelectTab={openPaneTab}
            onClose={closePane}
            onSelectVersion={selectVersion}
          />
        ) : null}
      </div>
      {search.version !== undefined ? (
        <PageVersionOverlay
          versionId={search.version}
          snapshot={snapshot.data}
          failed={snapshot.isError}
          onClose={closeVersion}
        />
      ) : null}
    </div>
  );
}
