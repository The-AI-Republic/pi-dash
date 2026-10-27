// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pages detail shell (parity/specs/pages.md §4, §6.5–§6.9, §7, §8.1–§8.3,
// §8.6, §9.6, PAGE-022–025, 045–050, 055–064, 074, 079–080, 084/086). One
// record read by URL id with visit
// tracking plus the shared collection read that feeds the sibling switcher;
// the header cluster with its action controls and options menu, the
// not-found panel, the centered record spinner, the title and body
// surfaces, the navigation pane with its live outline/statistics/
// assets/attribution, and the version overlay. The collaboration client mounts
// here in a later slice; this shell owns their seams and the URL protocol
// they share.

import { queryOptions, useQuery, useQueryClient } from "@tanstack/react-query";
import { listPages, PAGE_ACCESS_PRIVATE, type PageVersionDetail } from "@pidash/api-client";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS } from "../../../core/query/client.js";
import { edition, isPageAssistantEnabled } from "../../../core/edition/index.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { descriptionAssetSrc } from "../../../shared/editor/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { RichTextView } from "../../../shared/editor/RichTextView.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { useAppInstanceBootstrap } from "../../instance/index.js";
import { usePaletteWorkspaceMembers } from "../../../shared/shell/contextual/index.js";
import { isProjectAdmin, isProjectAuthorized } from "../../projects/index.js";
import { usePageExportDialog, type PageExportHost } from "../../pages-export/index.js";
import { pageDetailQueryOptions, pageVersionQueryOptions, pageVersionsQueryOptions } from "../api/detailQueries.js";
import { usePageMark, usePageRename } from "../api/detailMutations.js";
import { useDuplicatePage } from "../api/mutations.js";
import { useAddSubPage } from "../api/nestedMutations.js";
import { usePageArchiveWrite, usePageDeleteWrite, usePageVisibilityWrite } from "../api/rowMutations.js";
import { pagesListQueryOptions } from "../api/queries.js";
import { pageKeys } from "../api/keys.js";
import { pageForestQueryOptions } from "../api/treeQueries.js";
import { isPageBodyReady, markPageBodyReady } from "../lib/pageBodyCache.js";
import { getPageBodyDraft } from "../lib/pageBodyDraft.js";
import { pageBodyMarkdown } from "../lib/pageMarkup.js";
import { usePageSaveFallback } from "../lib/pageSaveFallback.js";
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
import { notifyError, notifySuccess } from "../../../shared/shell/toaster.js";
import { applyPageRestore, PAGE_VERSION_RESTORE_FAILED, PAGE_VERSION_RESTORE_SUCCESS } from "../lib/pageRestore.js";
import { snapshotBodyHtml } from "../lib/versionContent.js";
import { pageAncestors, pageDeleteKnownCount } from "../lib/pageNested.js";
import { quickCreateAccess } from "../lib/pageCreate.js";
import { copyPageLink } from "../lib/share.js";
import { pageAssistantReady } from "../lib/pageAssistant.js";
import { usePageSyncStatus } from "../lib/pageSync.js";
import { usePageLiveSession, usePageReadOnlySyncReport } from "../lib/pageLiveSession.js";
import { PageBodySkeleton, PageDetailSpinner } from "./PageDetailLoading.js";
import { PageDeleteDialog } from "./PageDeleteDialog.js";
import { PageDetailPanel } from "./PageDetailPanel.js";
import { PageDetailHeader } from "./PageDetailHeader.js";
import { PageFloatingPaneToggle } from "./PageFloatingPaneToggle.js";
import { PageLiveLoader } from "./PageLiveLoader.js";
import { PageCanvasHeader } from "./PageCanvasHeader.js";
import type { PageEditorHandle } from "../lib/pageEditorAdapter.js";
import { PageEditorSurface, PAGE_BODY_LABEL } from "./PageEditorSurface.js";
import { PageHeadingPreview } from "./PageHeadingPreview.js";
import { PageNavigationPane } from "./PageNavigationPane.js";
import { PageVersionOverlay } from "./PageVersionOverlay.js";
import { pageDetailHref, pageTabHref, PAGE_UNTITLED } from "../lib/pageList.js";
import { formattingBarVisible, pageCanvasWidth, usePagePreferences } from "../lib/pagePreferences.js";
import { useLivePageDocument } from "../lib/pageDocumentLive.js";
import { pageActorMembers, pageActorRows, type PageDocument } from "../lib/pagePane.js";
import { resetMentionDirectory, setMentionDirectory } from "../../../shared/editor/mentionDirectory.js";
import { subscribeEntityPeek, type EntityPeekRequest } from "../../../shared/shell/entityPeek.js";

// The peek host loads on first use: the page shell must not pull the
// issues area into its initial chunk (§13.2, Quality gates).
const LazyIssuePeek = React.lazy(() =>
  import("../../issues/peek/ResolvingIssuePeek.js").then((module) => ({ default: module.ResolvingIssuePeek }))
);

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
  const queryClient = useQueryClient();
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
  // One read of the address's version key: the overlay and both of its
  // actions below work from this narrowed value, never from a re-read.
  const openVersionId = search.version;
  const snapshot = useQuery({
    ...pageVersionQueryOptions(client, workspaceSlug, projectId, pageId, openVersionId ?? ""),
    enabled: openVersionId !== undefined && record.isSuccess,
  });

  const listRow = pages.data?.find((row) => row.id === pageId);
  const page = record.data;
  const hasAncestor = page !== undefined && (page.parent_id ?? page.parent) !== null;
  // Breadcrumbs walk the whole visible forest: a middle ancestor is never in
  // the top-level collection, and a parent the caller cannot see is absent
  // entirely, which is exactly what the restricted fallback reads. The walk
  // is deep because `sub_pages_count` hides an archived middle ancestor
  // under a live parent (§11.7); it reuses the collection as its roots and
  // shares the discovery cache entry.
  const forest = useQuery({
    ...pageForestQueryOptions(client, workspaceSlug, projectId, hasAncestor, { roots: pages.data, deep: true }),
    enabled: hasAncestor && pages.isSuccess && project.isSuccess && project.data.page_view !== false,
  });
  const role = roles.data?.[projectId] ?? null;
  const viewerId = me.data?.id ?? null;
  const instance = useAppInstanceBootstrap();

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
  // The assistant handle mounts only where the edition declares it, the
  // viewer may edit the page, and the instance has an LLM configuration
  // to answer with (PAGE-075). The community build fails the first
  // input and renders no assistant surface.
  const assistantReady = pageAssistantReady({
    editionEnabled: isPageAssistantEnabled(edition),
    authorized: capabilities?.canEdit === true,
    configured: instance.data?.bootstrap?.config.has_llm_configured === true,
  });

  // The attribution rows cannot say who edited or created the page until
  // the member directory answers; waiting for it here keeps the fallback
  // label for departed members from flashing on every load (PAGE-050).
  const directory = usePaletteWorkspaceMembers(workspaceSlug);
  // The timeline names each snapshot's author from the same directory the
  // attribution rows use (PAGE-051).
  const versionMembers = pageActorMembers(directory.data);

  // Mentions resolve their display name from this same directory (PAGE-041).
  React.useEffect(() => {
    if (directory.data === undefined) return;
    setMentionDirectory(
      directory.data.map((member) => ({ id: member.member.id, name: member.member.display_name ?? "" })),
      viewerId
    );
    return resetMentionDirectory;
  }, [directory.data, viewerId]);

  // The page owns no peek surface; it emits a request and lets the shell's
  // issues peek host open the matching item (PAGE-043, §13.2).
  const [entityPeek, setEntityPeek] = React.useState<EntityPeekRequest | null>(null);
  React.useEffect(
    () =>
      subscribeEntityPeek((request) => {
        if (request.entityName === "issue" && request.projectId !== null) setEntityPeek(request);
      }),
    []
  );

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
  // One collaboration session per editable page (PAGE-065–067): the
  // mount below mirrors the document the live editors bind. Read-only
  // pages keep their stored snapshot and report converged instead.
  const liveMount = usePageLiveSession({
    workspaceSlug,
    projectId,
    pageId,
    user: me.data === undefined ? null : { id: me.data.id, name: me.data.display_name },
    enabled: (capabilities?.canEdit ?? false) && page !== undefined,
  });
  usePageReadOnlySyncReport(pageId, page !== undefined && capabilities !== null && !capabilities.canEdit);
  // The binding is stable per document: a fresh object every render
  // would rebuild the editors' extensions and drop the caret.
  const liveDocHandle = liveMount?.docHandle ?? null;
  const liveDoc = React.useMemo(() => (liveDocHandle === null ? null : { doc: liveDocHandle }), [liveDocHandle]);
  // With no session (signed out) or a known document the surface
  // renders; the loader holds the canvas only while the document is
  // still unknown (PAGE-067).
  const liveReady = liveMount === null || liveMount.liveReady;
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
  // The ancestor trail walks the same cached collection the sibling
  // switcher uses (PAGE-086), plus the open record itself; an ancestor the
  // caller never received renders as the restricted fallback.
  const trailPool = React.useMemo(() => {
    const visible = [...(forest.data?.rows ?? []), ...(pages.data ?? [])];
    if (page === undefined) return visible;
    return [page, ...visible.filter((row) => row.id !== page.id)];
  }, [page, forest.data, pages.data]);
  // The trail renders once the ancestor walk settles: a pending read must
  // never flash the restricted fallback for an ancestor it simply has not
  // loaded yet (PAGE-086).
  const ancestorsReady = !hasAncestor || forest.isSuccess;
  const ancestors = React.useMemo(
    () => (page === undefined || !ancestorsReady ? [] : pageAncestors(trailPool, page.id)),
    [trailPool, page, ancestorsReady]
  );
  const addSubPage = useAddSubPage(workspaceSlug, projectId);
  // The options menu drives the same shared writes as the list row menu
  // (PAGE-057/059/060): duplication, visibility, shelve/restore and delete.
  // The lock control and the star own their hooks inside the header.
  const headerTarget = { workspaceSlug, projectId, page: { id: pageId, name: page?.name ?? "" } };
  const duplicate = useDuplicatePage(workspaceSlug, projectId);
  const visibility = usePageVisibilityWrite(headerTarget);
  const archive = usePageArchiveWrite(headerTarget);
  const deletion = usePageDeleteWrite(headerTarget);
  const [deleteOpen, setDeleteOpen] = React.useState(false);
  // A shelved page never reports its subtree size through the row, so the
  // confirmation reads the direct children once the dialog is actually open
  // (PAGE-089) — the same shape the connected row uses.
  const knownDeleteCount = page === undefined ? undefined : pageDeleteKnownCount(page);
  const deleteChildren = useQuery(
    queryOptions({
      queryKey: pageKeys.children(workspaceSlug, projectId, pageId, true),
      queryFn: () => listPages(getAppClient(), workspaceSlug, projectId, { parent: pageId, includeArchived: true }),
      staleTime: LIST_STALE_TIME_MS,
      enabled: deleteOpen && knownDeleteCount === undefined,
    })
  );
  const deleteCount = knownDeleteCount ?? deleteChildren.data?.length;

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

  // The four display preferences are local (spec §9.1, PAGE-072): the
  // store is read on mount and on every page switch, and a change here
  // reflows this canvas immediately. None of it reaches the record.
  const { preferences, setPreference } = usePagePreferences(pageId);
  const showBar =
    capabilities !== null &&
    formattingBarVisible({ editable: capabilities.canEdit, stickyToolbar: preferences.sticky_toolbar });
  const canvasWidth = pageCanvasWidth({ fullWidth: preferences.full_width, paneOpen });
  const resolveAsset = React.useCallback(
    (src: string) => descriptionAssetSrc(src, { workspaceSlug, projectId }),
    [workspaceSlug, projectId]
  );

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

  // A failed snapshot read keeps its address; asking again drops the
  // rejected answer so the read flies afresh and the skeleton returns
  // (PAGE-054). Only a mounted overlay can ask, so it always names one.
  const retryVersion = (versionId: string): void => {
    void queryClient.invalidateQueries({
      queryKey: pageKeys.version(workspaceSlug, projectId, pageId, versionId),
    });
  };

  // Restore is the live document taking the snapshot's body: the editor
  // reports the write through its own update, so the draft seam and the
  // pane see it as a typed edit and the reader never leaves the page
  // (PAGE-053). Without a live editor there is nothing to write into and
  // the reader is told so instead.
  const editorHandle = React.useRef<PageEditorHandle | null>(null);
  const onEditorHandle = React.useCallback((handle: PageEditorHandle | null) => {
    editorHandle.current = handle;
  }, []);
  const restoreVersion = React.useCallback(
    (opened: PageVersionDetail) => {
      if (!applyPageRestore(editorHandle.current, snapshotBodyHtml(opened))) {
        notifyError(PAGE_VERSION_RESTORE_FAILED);
        return;
      }
      notifySuccess(PAGE_VERSION_RESTORE_SUCCESS);
      // The restore is a new save, so the history and its timeline are
      // stale from here on.
      void queryClient.invalidateQueries({ queryKey: pageKeys.versions(workspaceSlug, projectId, pageId) });
      closeVersion();
    },
    [queryClient, workspaceSlug, projectId, pageId, closeVersion]
  );

  // The newest local body, whatever surface holds it: the live editor
  // when one is mounted, otherwise the session draft, otherwise the stored
  // snapshot (PAGE-064, PAGE-074).
  const readLiveHtml = (): string => editorHandle.current?.html() ?? getPageBodyDraft(pageId) ?? storedHtml;
  // The socket-loss fallback (PAGE-064) owns body persistence while the
  // sync state reports the fault badge on an editable page; its manual
  // save is the surface's save-gesture handler.
  const saveFallback = usePageSaveFallback({
    workspaceSlug,
    projectId,
    pageId,
    title: page?.name ?? "",
    active: (capabilities?.canEdit ?? false) && syncStatus === "lost",
    readHtml: readLiveHtml,
  });
  // The export entry (PAGE-074) appears only once the exports area
  // registers its dialog; with no registration the menu has no entry.
  const ExportDialog = usePageExportDialog();
  const [exportOpen, setExportOpen] = React.useState(false);

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
    members: versionMembers,
  });
  // The export dialog reads the live body through these getters (PAGE-074).
  const exportHost: PageExportHost = {
    pageTitle,
    projectId,
    workspaceSlug,
    getHtml: readLiveHtml,
    getMarkdown: () => pageBodyMarkdown(readLiveHtml()),
  };

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
        ancestors={ancestors}
        siblings={siblings}
        syncStatus={syncStatus}
        editable={capabilities.canEdit}
        showStar={isProjectAuthorized(role)}
        showLock={canManage}
        canDuplicate={isProjectAuthorized(role)}
        canManage={canManage}
        onSelectSibling={(id) => goTo(siblingHref(workspaceSlug, projectId, id))}
        onCopyLink={() => {
          void copyPageLink(workspaceSlug, projectId, page.id);
        }}
        onOpenHistory={() => onSearchChange(historySearch(search))}
        onDuplicate={() => duplicate.mutate({ pageId: page.id })}
        onToggleArchive={() => archive.toggleArchive(page.archived_at === null)}
        onToggleVisibility={() => visibility.setVisibility(page.access !== PAGE_ACCESS_PRIVATE)}
        onRequestDelete={() => setDeleteOpen(true)}
        onOpenPane={openPane}
        paneOpen={paneOpen}
        preferences={preferences}
        onPreferenceChange={setPreference}
        {...(ExportDialog === null
          ? {}
          : {
              onExport: () => {
                setExportOpen(true);
              },
            })}
        {...(isProjectAuthorized(role) && !capabilities.isShelved
          ? {
              onAddSubPage: () => {
                addSubPage.mutate(
                  // The detail has no tab; use PAGE-007's open-tab default
                  // rather than inheriting the source page's visibility.
                  { parentId: page.id, access: quickCreateAccess("public") },
                  { onSuccess: (created) => goTo(pageDetailHref(workspaceSlug, projectId, created.id)) }
                );
              },
            }
          : {})}
      />
      {ExportDialog !== null && exportOpen ? (
        <ExportDialog host={exportHost} open onClose={() => setExportOpen(false)} />
      ) : null}
      <div className="flex min-h-0 flex-1">
        {/* The column isolates its own stacking so the snapshot overlay's
            layer never climbs above the page header beside it: the header
            stays reachable while a snapshot covers the canvas (§4.5). */}
        <div data-testid="page-canvas-stage" className="relative isolate flex min-h-0 min-w-0 flex-1">
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
                style={{ maxWidth: canvasWidth }}
              >
                {capabilities.isShelved || capabilities.isLocked ? (
                  <p className="text-caption mb-(--space-4) text-(--text-muted)">
                    {capabilities.isShelved
                      ? "This page is archived and read-only."
                      : "This page is locked and read-only."}
                  </p>
                ) : null}
                {capabilities.canEdit ? (
                  liveReady ? (
                    <PageEditorSurface
                      pageId={page.id}
                      workspaceSlug={workspaceSlug}
                      projectId={projectId}
                      name={page.name}
                      descriptionHtml={page.description_html}
                      descriptionJson={page.description_json}
                      logoProps={page.logo_props as Record<string, unknown>}
                      onRename={saveName}
                      onMarkPick={saveMark}
                      showBar={showBar}
                      onSave={saveFallback.requestManualSave}
                      fullWidth={preferences.full_width}
                      fontSize={preferences.font_size}
                      fontStyle={preferences.font_style}
                      bodyPending={!bodyReady}
                      onHandle={onEditorHandle}
                      collab={liveDoc}
                      seedDocument={liveMount?.seedDocument ?? false}
                      assistantReady={assistantReady}
                    />
                  ) : (
                    <PageLiveLoader />
                  )
                ) : (
                  <>
                    <PageCanvasHeader
                      title={{ pageId: page.id, name: page.name, editable: false }}
                      fontSize={preferences.font_size}
                      fontStyle={preferences.font_style}
                      logoProps={page.logo_props as Record<string, unknown>}
                      editable={false}
                      titleEmpty={page.name.trim() === ""}
                      onMarkPick={saveMark}
                    />
                    {bodyReady ? (
                      <div
                        className={`editor-container text-size-${preferences.font_size} text-face-${preferences.font_style}`}
                      >
                        <RichTextView
                          html={page.description_html}
                          label={PAGE_BODY_LABEL}
                          resolveAsset={resolveAsset}
                          workspaceSlug={workspaceSlug}
                        />
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
          {/* The bar's absence is what surfaces the floating toggle
              (PAGE-035); when the bar is there, this never mounts. */}
          {showBar || paneOpen ? null : <PageFloatingPaneToggle onOpen={openPane} />}
          {/* The snapshot overlay covers this canvas column only, so the
              page header above and the pane beside it stay usable (§4.5,
              PAGE-052). */}
          {openVersionId === undefined ? null : (
            <PageVersionOverlay
              versionId={openVersionId}
              snapshot={snapshot.data}
              failed={snapshot.isError}
              onClose={closeVersion}
              onRetry={() => retryVersion(openVersionId)}
              restore={capabilities.canEdit ? { onRestore: restoreVersion } : null}
            />
          )}
        </div>
        <PageNavigationPane
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          pageId={page.id}
          search={search}
          document={paneDocument}
          actors={actors}
          versions={search.paneTab === "info" ? versions.data : undefined}
          versionMembers={versionMembers}
          onSelectTab={openPaneTab}
          onClose={closePane}
          onSelectVersion={selectVersion}
          onSelectCurrentVersion={closeVersion}
          onSelectHeading={live.scrollToHeading}
        />
      </div>
      <PageDeleteDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        pageName={page.name}
        {...(deleteCount === undefined ? {} : { rehomedCount: deleteCount })}
        pending={deletion.pending}
        onConfirm={() => {
          deletion.remove({
            onDeleted: () => {
              setDeleteOpen(false);
              goTo(pageTabHref(workspaceSlug, projectId, "archived"));
            },
          });
        }}
      />
      {entityPeek === null ? null : (
        <React.Suspense fallback={null}>
          <LazyIssuePeek
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            projectIdentifier={project.data?.identifier ?? ""}
            peeked={entityPeek.entityIdentifier}
            peekProjectId={entityPeek.projectId}
            onClose={() => setEntityPeek(null)}
          />
        </React.Suspense>
      )}
    </div>
  );
}
