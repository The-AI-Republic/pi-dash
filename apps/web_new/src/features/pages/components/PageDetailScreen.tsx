// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages detail shell (parity/specs/pages.md §4, §6.5–§6.9, PAGE-022–025,
// 045–050, 079–080). One record read by URL id with visit tracking plus the
// shared collection read that feeds the sibling switcher; the header
// cluster, the not-found panel, the centered record spinner, the title and
// body surfaces, the navigation pane with its live outline/statistics/
// assets/attribution, and the version overlay. The editor, toolbar, header
// menu entries and collaboration client mount here in later slices; this
// shell owns their seams and the URL protocol they share.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { RichTextView } from "../../../shared/editor/RichTextView.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { usePaletteWorkspaceMembers } from "../../../shared/shell/contextual/index.js";
import { isProjectAdmin, isProjectAuthorized } from "../../projects/index.js";
import { pageDetailQueryOptions, pageVersionQueryOptions, pageVersionsQueryOptions } from "../api/detailQueries.js";
import { usePageMark, usePageRename } from "../api/detailMutations.js";
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
import { PageCanvasHeader } from "./PageCanvasHeader.js";
import { PageEditorSurface, PAGE_BODY_LABEL } from "./PageEditorSurface.js";
import { PageHeadingPreview } from "./PageHeadingPreview.js";
import { PageNavigationPane } from "./PageNavigationPane.js";
import { PageVersionOverlay } from "./PageVersionOverlay.js";
import { PAGE_UNTITLED } from "../lib/pageList.js";
import { useLivePageDocument } from "../lib/pageDocumentLive.js";
import { PAGE_CANVAS_MEASURE_PX, pageActorMembers, pageActorRows, type PageDocument } from "../lib/pagePane.js";

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

  // The attribution rows cannot say who edited or created the page until
  // the member directory answers; waiting for it here keeps the fallback
  // label for departed members from flashing on every load (PAGE-050).
  const directory = usePaletteWorkspaceMembers(workspaceSlug);

  const body = resolvePageDetailBody({
    pending: record.isPending || me.isPending || roles.isPending || directory.isPending,
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
  const mark = usePageMark(workspaceSlug, projectId, pageId);
  // One save path for the header's mark, on editable and read-only pages
  // alike: a read-only header simply has no picker that could reach it.
  const saveMark = React.useCallback(
    (draft: Record<string, unknown>) => {
      mark.mutate(draft);
    },
    [mark]
  );
  // The rename path hands the title's committed value straight to the
  // mutation; `mutate` is stable, so the header does not re-render for it.
  const saveName = rename.mutate;

  // The pane reads the canvas, so headings, counts and assets follow
  // typing; before the canvas paints, the stored body stands in (PAGE-046).
  const canvasRef = React.useRef<HTMLDivElement | null>(null);
  const storedHtml = page?.description_html ?? "";
  const live = useLivePageDocument(canvasRef, storedHtml, { workspaceSlug, projectId });
  const paneDocument = live.document;
  const openPane = (): void => {
    onSearchChange(paneSearch(search, "outline"));
  };
  const openFromPreview = (heading: PageDocument["headings"][number] | null): void => {
    onSearchChange(paneSearch(search, "outline"));
    if (heading !== null) live.scrollToHeading(heading);
  };

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
  const actors = pageActorRows({
    workspaceSlug,
    updatedBy: page.updated_by ?? page.created_by,
    updatedAt: page.updated_at,
    ownedBy: page.owned_by,
    createdAt: page.created_at,
    members: pageActorMembers(directory.data),
  });

  return (
    // The screen owns its own height and clips horizontally: the pane's
    // shut state is a negative right margin, which must stay out of the
    // layout's scrollport instead of becoming reachable by scrolling the
    // content area (PAGE-045). The canvas column below is the page's
    // vertical scroller.
    <div data-testid="page-detail-screen" className="flex h-full min-h-0 flex-col overflow-x-clip">
      <PageDetailHeader
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName}
        projectLogo={(project.data?.logo_props ?? {}) as Record<string, unknown>}
        projectIdentifier={project.data?.identifier ?? ""}
        page={page}
        siblings={siblings}
        syncStatus={syncStatus}
        showStar={isProjectAuthorized(role)}
        onSelectSibling={(id) => goTo(siblingHref(workspaceSlug, projectId, id))}
        onCopyLink={() => {
          void copyPageLink(workspaceSlug, projectId, page.id);
        }}
        onOpenHistory={() => onSearchChange(historySearch(search))}
        onOpenPane={openPane}
        paneOpen={paneOpen}
      />
      <div className="flex min-h-0 flex-1">
        <div data-testid="page-canvas-stage" className="relative flex min-h-0 min-w-0 flex-1">
          <div data-testid="page-canvas-scroll" className="min-h-0 min-w-0 flex-1 overflow-y-auto">
            {/* The measured canvas fills the column, so the pane's slide
              reflows it by the pane's own width (PAGE-045). The readable
              measure lives inside it and follows the pane state: narrow
              while it is shut, wide while it is open (§6.5). The title is
              part of the canvas, which is why the pane's live read scopes
              itself to the body below it. */}
            <article
              id="page-content-container"
              aria-label={pageTitle}
              ref={canvasRef}
              className="relative w-full py-(--space-6)"
            >
              <div
                className="frame-renderer mx-auto w-full px-(--space-8) transition-[max-width] duration-(--motion-duration) ease-(--motion-easing)"
                style={{ maxWidth: paneOpen ? PAGE_CANVAS_MEASURE_PX.wide : PAGE_CANVAS_MEASURE_PX.readable }}
              >
                {capabilities.isShelved || capabilities.isLocked ? (
                  <p className="text-caption mb-(--space-4) text-(--text-muted)">
                    {capabilities.isShelved
                      ? "This page is archived and read-only."
                      : "This page is locked and read-only."}
                  </p>
                ) : null}
                {capabilities.canEdit ? (
                  <PageEditorSurface
                    pageId={page.id}
                    name={page.name}
                    descriptionHtml={page.description_html}
                    logoProps={page.logo_props as Record<string, unknown>}
                    onRename={saveName}
                    onMarkPick={saveMark}
                    showBar
                    bodyPending={!bodyReady}
                  />
                ) : (
                  <>
                    <PageCanvasHeader
                      title={{ pageId: page.id, name: page.name, editable: false }}
                      logoProps={page.logo_props as Record<string, unknown>}
                      editable={false}
                      titleEmpty={page.name.trim() === ""}
                      onMarkPick={saveMark}
                    />
                    {bodyReady ? (
                      <div className="editor-container">
                        <RichTextView html={page.description_html} label={PAGE_BODY_LABEL} />
                      </div>
                    ) : (
                      <PageBodySkeleton />
                    )}
                  </>
                )}
                {canManage ? <span data-testid="page-detail-manage-mount" hidden aria-hidden="true" /> : null}
                {bodyReady ? <span data-testid="page-detail-action-mount" hidden aria-hidden="true" /> : null}
              </div>
            </article>
          </div>
          {/* Sits beside the scroller rather than inside it, so it stays at
              the canvas edge however far the body has scrolled (PAGE-046). */}
          {paneOpen ? null : <PageHeadingPreview headings={paneDocument.headings} onOpen={openFromPreview} />}
        </div>
        <PageNavigationPane
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          pageId={page.id}
          search={search}
          document={paneDocument}
          actors={actors}
          versions={search.paneTab === "info" ? versions.data : undefined}
          onSelectTab={openPaneTab}
          onClose={closePane}
          onSelectVersion={selectVersion}
          onSelectHeading={live.scrollToHeading}
        />
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
