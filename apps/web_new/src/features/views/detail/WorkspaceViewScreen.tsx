// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace view detail (parity/specs/views.md §11, VIEW-038/040). One
// screen for the four static defaults and the customs: the header
// switcher spans both, the body is the workspace issue list scoped to
// the view (a default's static condition or the custom's stored
// expression), and unknown, forbidden or blank records settle into the
// same missing-view state as the project detail. The layout selector is
// cloud-only: OSS renders the spreadsheet body with no selector.

import { updateWorkspaceView, type IssueListQuery, type SavedView, type SavedViewList } from "@pidash/api-client";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/index.js";
import { edition } from "../../../core/edition/index.js";
import { useMe, usePermissions, useWorkspace } from "../../../core/session/index.js";
import {
  DisplayControl,
  GlobalIssuesScreen,
  isLayoutKind,
  LazyIssueFilters,
  LayoutSwitcher,
  ListSkeleton,
  orderByFromDisplayFilters,
  parseDisplayPrefs,
  SavedViewEmptyCard,
  selectCreatableProjects,
  selectDisplayPrefs,
  useEntityPrefs,
  useUpdateDisplayPrefs,
  withRichFilters,
  type DisplayPrefs,
  type IssuesEntity,
  type LayoutKind,
  type RichExpression,
  type WorkItemCreateProject,
} from "../../issues/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { notifyError, useDocumentHead } from "../../../shared/shell/index.js";
import { viewKeys } from "../api/keys.js";
import { workspaceViewDetailQueryOptions } from "../api/detail.js";
import { workspaceViewsQueryOptions } from "../api/queries.js";
import { ProjectViewsError } from "../components/ProjectViewsError.js";
import { WorkspaceViewsTabStrip } from "../components/WorkspaceViewsTabStrip.js";
import type { WorkspaceViewMenuStanding } from "../components/WorkspaceViewMenu.js";
import { DeleteWorkspaceViewDialog } from "../dialog/DeleteWorkspaceViewDialog.js";
import { failureDetail } from "../dialog/failureDetail.js";
import { WORKSPACE_VIEW_ORDER_BY } from "../dialog/workspaceDisplay.js";
import { WorkspaceViewDialog } from "../dialog/WorkspaceViewDialog.js";
import {
  workspaceCustomViewHref,
  workspaceDefaultViewHref,
  workspaceViewsTitle,
  type WorkspaceDefaultView,
} from "../lib/workspaceViews.js";
import { MISSING_WORKSPACE_VIEW_BACK_LABEL } from "./model.js";
import { SavedViewMissingState } from "./SavedViewMissingState.js";
import { WorkspaceDefaultFilters } from "./WorkspaceDefaultFilters.js";
import { WorkspaceViewHeader } from "./WorkspaceViewHeader.js";
import {
  findVisibleCustomView,
  findWorkspaceDefaultView,
  offeredWorkspaceLayouts,
  resolveWorkspaceBodyLayout,
  workspaceActiveLayout,
  workspaceDefaultStaticFilters,
  workspaceLayoutPatch,
} from "./workspaceDetail.js";
import type { WorkspaceViewDetailParams } from "./workspacePreload.js";

const LazyWorkItemCreateDialog = React.lazy(() =>
  import("../../issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);

const LAYOUT_FAILURE_TITLE = "Couldn't switch the layout";
const LAYOUT_FAILURE_BODY = "The view is unchanged — check your connection and try again.";

export interface WorkspaceViewScreenProps {
  params: WorkspaceViewDetailParams;
}

function WorkspaceDetailSkeleton(): React.ReactElement {
  return (
    <section aria-label="Workspace view" className="flex min-h-0 flex-1 flex-col">
      <ListSkeleton />
    </section>
  );
}

function WorkspaceDetailError({ onRetry }: { onRetry: () => void }): React.ReactElement {
  return (
    <section aria-label="Workspace view" className="flex min-h-0 flex-1 flex-col">
      <ProjectViewsError onRetry={onRetry} />
    </section>
  );
}

export function WorkspaceViewScreen({ params }: WorkspaceViewScreenProps): React.ReactElement {
  const client = getAppClient();
  const navigate = useNavigate();
  const { workspace } = useWorkspace(params.ws);
  const permissions = usePermissions(params.ws);
  const me = useMe();
  const customs = useQuery(workspaceViewsQueryOptions(client, params.ws));

  useDocumentHead({ title: workspaceViewsTitle(workspace?.name), robots: "noindex, nofollow" });

  const openView = (viewId: string): void => {
    const href =
      findWorkspaceDefaultView(viewId) === null
        ? workspaceCustomViewHref(params.ws, viewId)
        : workspaceDefaultViewHref(params.ws, viewId);
    void navigate({ to: href });
  };
  const openFallback = (): void => {
    void navigate({ to: workspaceDefaultViewHref(params.ws, "all-issues") });
  };

  const defaultView = findWorkspaceDefaultView(params.viewId);
  if (defaultView !== null) {
    return (
      <WorkspaceDefaultDetail
        workspaceSlug={params.ws}
        defaultView={defaultView}
        customs={customs.data ?? []}
        customsLoading={customs.isPending}
        customsFailed={customs.isError}
        meId={me.data?.id ?? null}
        meFailed={me.isError}
        onRetryMe={() => {
          void me.refetch();
        }}
        canEdit={permissions.canEdit}
        onOpenView={openView}
      />
    );
  }
  if (customs.status === "pending") return <WorkspaceDetailSkeleton />;
  if (customs.status === "error") {
    return (
      <WorkspaceDetailError
        onRetry={() => {
          void customs.refetch();
        }}
      />
    );
  }
  // Membership in the visible list is the access check: the retrieve
  // alone is not guest-narrowed, so an id the list hides never renders.
  const listed = findVisibleCustomView(customs.data, params.viewId);
  if (listed === null) {
    return <SavedViewMissingState onBack={openFallback} backLabel={MISSING_WORKSPACE_VIEW_BACK_LABEL} />;
  }
  return (
    <WorkspaceCustomDetail
      workspaceSlug={params.ws}
      viewId={params.viewId}
      customs={customs.data}
      meId={me.data?.id ?? null}
      canEdit={permissions.canEdit}
      isAdmin={permissions.isAdmin}
      onOpenView={openView}
      onOpenFallback={openFallback}
    />
  );
}

interface WorkspaceDefaultDetailProps {
  workspaceSlug: string;
  defaultView: WorkspaceDefaultView;
  customs: readonly SavedView[];
  customsLoading: boolean;
  customsFailed: boolean;
  meId: string | null;
  meFailed: boolean;
  onRetryMe: () => void;
  canEdit: boolean;
  onOpenView: (viewId: string) => void;
}

function WorkspaceDefaultDetail({
  workspaceSlug,
  defaultView,
  customs,
  customsLoading,
  customsFailed,
  meId,
  meFailed,
  onRetryMe,
  canEdit,
  onOpenView,
}: WorkspaceDefaultDetailProps): React.ReactElement {
  const client = getAppClient();
  const entity = React.useMemo<IssuesEntity>(() => ({ scope: "workspace" }), []);
  const prefs = useEntityPrefs(workspaceSlug, entity);
  const { updateDisplayPrefs } = useUpdateDisplayPrefs(workspaceSlug, entity);
  const projects = useQuery(projectsQueryOptions(client, workspaceSlug));
  const [liveExpression, setLiveExpression] = React.useState<RichExpression>({});
  const [createOpen, setCreateOpen] = React.useState(false);
  const [createWorkItemOpen, setCreateWorkItemOpen] = React.useState(false);

  const needsMe = defaultView.key !== "all-issues";
  if (needsMe && meId === null) {
    if (meFailed) return <WorkspaceDetailError onRetry={onRetryMe} />;
    return <WorkspaceDetailSkeleton />;
  }
  const staticFilters = meId === null ? null : workspaceDefaultStaticFilters(defaultView, meId);
  const storedPrefs = prefs.data === undefined ? null : selectDisplayPrefs(prefs.data);
  // Display reads are personal: the layout and order follow the caller's
  // workspace record, opening on the surface defaults until it arrives.
  // The layout reads the raw stored value: the parsed prefs default to
  // "list", which this surface cannot draw, while an absent value opens
  // the spreadsheet.
  const offered = offeredWorkspaceLayouts(edition);
  const displayOffered: readonly LayoutKind[] = offered.length > 0 ? offered : ["spreadsheet"];
  const storedLayout = prefs.data?.display_filters["layout"];
  const bodyLayout = resolveWorkspaceBodyLayout(storedLayout, offered);
  const activeLayout: LayoutKind = isLayoutKind(storedLayout) ? storedLayout : "spreadsheet";
  const orderBy = storedPrefs?.orderBy ?? WORKSPACE_VIEW_ORDER_BY;
  const bodyQuery: IssueListQuery = React.useMemo(
    () => withRichFilters({ orderBy, ...(staticFilters === null ? {} : { filters: staticFilters }) }, liveExpression),
    [orderBy, staticFilters, liveExpression]
  );
  const creatable: WorkItemCreateProject[] = React.useMemo(
    () =>
      projects.data === undefined
        ? []
        : selectCreatableProjects(
            projects.data.map((entry) => ({
              id: entry.id,
              name: entry.name,
              identifier: entry.identifier,
              memberRole: entry.member_role,
              archived: entry.archived_at !== null,
            }))
          ),
    [projects.data]
  );
  const isAllIssues = defaultView.key === "all-issues";
  const emptyState = isAllIssues ? undefined : (
    <SavedViewEmptyCard
      canAct={canEdit}
      onCreate={() => {
        setCreateWorkItemOpen(true);
      }}
    />
  );

  return (
    <WorkspaceDefaultFilters workspaceSlug={workspaceSlug} onExpressionChange={setLiveExpression}>
      {({ control, row }) => (
        <section aria-label="Workspace view" className="flex min-h-0 flex-1 flex-col">
          <WorkspaceViewHeader
            workspaceSlug={workspaceSlug}
            defaultView={defaultView}
            view={null}
            detailHref={workspaceDefaultViewHref(workspaceSlug, defaultView.key)}
            customs={customs}
            customsLoading={customsLoading}
            customsFailed={customsFailed}
            standing={{ isOwner: false, canDelete: false }}
            layoutControl={
              offered.length === 0 ? null : (
                <LayoutSwitcher
                  offered={offered}
                  active={activeLayout}
                  onSwitch={(layout) => {
                    updateDisplayPrefs({ layout });
                  }}
                />
              )
            }
            filterControl={control}
            displayControl={
              <DisplayControl
                workspaceSlug={workspaceSlug}
                entity={entity}
                offered={displayOffered}
                hideCycle
                hideModule
              />
            }
            canAddView={canEdit}
            onOpenView={onOpenView}
            onAddView={() => {
              setCreateOpen(true);
            }}
            onEdit={null}
            onDelete={null}
          />
          <WorkspaceViewsTabStrip workspaceSlug={workspaceSlug} activeViewId={defaultView.key} />
          {row}
          <div className="min-h-0 flex-1">
            {bodyLayout === null ? (
              <div data-testid="workspace-view-empty-body" className="min-h-0 flex-1" />
            ) : (
              <React.Suspense fallback={<ListSkeleton />}>
                <GlobalIssuesScreen query={bodyQuery} hideHeader emptyState={emptyState} label={defaultView.label} />
              </React.Suspense>
            )}
          </div>
          {createOpen ? (
            <WorkspaceViewDialog
              open={createOpen}
              onOpenChange={setCreateOpen}
              workspaceSlug={workspaceSlug}
              mode="create"
            />
          ) : null}
          {createWorkItemOpen ? (
            <React.Suspense fallback={null}>
              <LazyWorkItemCreateDialog
                open={createWorkItemOpen}
                onOpenChange={setCreateWorkItemOpen}
                workspaceSlug={workspaceSlug}
                projects={creatable}
              />
            </React.Suspense>
          ) : null}
        </section>
      )}
    </WorkspaceDefaultFilters>
  );
}

interface WorkspaceCustomDetailProps {
  workspaceSlug: string;
  viewId: string;
  customs: readonly SavedView[];
  meId: string | null;
  canEdit: boolean;
  isAdmin: boolean;
  onOpenView: (viewId: string) => void;
  onOpenFallback: () => void;
}

function WorkspaceCustomDetail({
  workspaceSlug,
  viewId,
  customs,
  meId,
  canEdit,
  isAdmin,
  onOpenView,
  onOpenFallback,
}: WorkspaceCustomDetailProps): React.ReactElement {
  const client = getAppClient();
  const detail = useQuery(workspaceViewDetailQueryOptions(client, workspaceSlug, viewId));
  if (detail.isPending) return <WorkspaceDetailSkeleton />;
  if (detail.isError) {
    return <SavedViewMissingState onBack={onOpenFallback} backLabel={MISSING_WORKSPACE_VIEW_BACK_LABEL} />;
  }
  return (
    <WorkspaceCustomBody
      workspaceSlug={workspaceSlug}
      view={detail.data}
      customs={customs}
      meId={meId}
      canEdit={canEdit}
      isAdmin={isAdmin}
      onOpenView={onOpenView}
      onOpenFallback={onOpenFallback}
    />
  );
}

interface WorkspaceCustomBodyProps {
  workspaceSlug: string;
  view: SavedView;
  customs: readonly SavedView[];
  meId: string | null;
  canEdit: boolean;
  isAdmin: boolean;
  onOpenView: (viewId: string) => void;
  onOpenFallback: () => void;
}

function WorkspaceCustomBody({
  workspaceSlug,
  view,
  customs,
  meId,
  canEdit,
  isAdmin,
  onOpenView,
  onOpenFallback,
}: WorkspaceCustomBodyProps): React.ReactElement {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const entity = React.useMemo<IssuesEntity>(() => ({ scope: "workspace" }), []);
  const projects = useQuery(projectsQueryOptions(client, workspaceSlug));
  const [display, setDisplay] = React.useState<DisplayPrefs>(() =>
    parseDisplayPrefs(view.display_filters, view.display_properties)
  );
  React.useEffect(() => {
    setDisplay(parseDisplayPrefs(view.display_filters, view.display_properties));
  }, [view]);
  const [createOpen, setCreateOpen] = React.useState(false);
  const [editing, setEditing] = React.useState(false);
  const [deleting, setDeleting] = React.useState(false);
  const [createWorkItemOpen, setCreateWorkItemOpen] = React.useState(false);
  const switchInFlight = React.useRef(false);

  const adoptUpdatedView = (updated: SavedView): void => {
    queryClient.setQueryData(viewKeys.workspaceDetail(workspaceSlug, updated.id), updated);
    queryClient.setQueryData<SavedViewList>(viewKeys.workspaceList(workspaceSlug), (current) =>
      current === undefined ? current : current.map((row) => (row.id === updated.id ? updated : row))
    );
  };
  const adoptCreatedView = (created: SavedView): void => {
    queryClient.setQueryData(viewKeys.workspaceDetail(workspaceSlug, created.id), created);
    queryClient.setQueryData<SavedViewList>(viewKeys.workspaceList(workspaceSlug), (current) =>
      current === undefined ? [created] : [created, ...current]
    );
  };
  const switchLayout = (layout: LayoutKind): void => {
    if (switchInFlight.current) return;
    switchInFlight.current = true;
    void (async () => {
      try {
        adoptUpdatedView(await updateWorkspaceView(client, workspaceSlug, view.id, workspaceLayoutPatch(view, layout)));
      } catch (error) {
        notifyError(LAYOUT_FAILURE_TITLE, failureDetail(error, LAYOUT_FAILURE_BODY));
      } finally {
        switchInFlight.current = false;
      }
    })();
  };

  const offered = offeredWorkspaceLayouts(edition);
  const displayOffered: readonly LayoutKind[] = offered.length > 0 ? offered : ["spreadsheet"];
  const locked = view.is_locked;
  // A locked view is read-only: the selector, the toggle and the Display
  // control hide, and the row's draft could never persist, so no row mounts.
  const bodyLayout = resolveWorkspaceBodyLayout(view.display_filters["layout"], offered);
  const bodyQuery: IssueListQuery = React.useMemo(
    () => withRichFilters({ orderBy: orderByFromDisplayFilters(view.display_filters) }, view.rich_filters),
    [view]
  );
  const creatable: WorkItemCreateProject[] = React.useMemo(
    () =>
      projects.data === undefined
        ? []
        : selectCreatableProjects(
            projects.data.map((entry) => ({
              id: entry.id,
              name: entry.name,
              identifier: entry.identifier,
              memberRole: entry.member_role,
              archived: entry.archived_at !== null,
            }))
          ),
    [projects.data]
  );
  const isOwner = meId !== null && meId === view.owned_by;
  const standing: WorkspaceViewMenuStanding = { isOwner, canDelete: isOwner || isAdmin };
  const detailHref = workspaceCustomViewHref(workspaceSlug, view.id);

  const header = (filterControl: React.ReactNode): React.ReactElement => (
    <WorkspaceViewHeader
      workspaceSlug={workspaceSlug}
      defaultView={null}
      view={view}
      detailHref={detailHref}
      customs={customs}
      customsLoading={false}
      customsFailed={false}
      standing={standing}
      layoutControl={
        offered.length === 0 || locked ? null : (
          <LayoutSwitcher offered={offered} active={workspaceActiveLayout(view)} onSwitch={switchLayout} />
        )
      }
      filterControl={locked ? null : filterControl}
      displayControl={
        locked ? null : (
          <DisplayControl
            workspaceSlug={workspaceSlug}
            entity={entity}
            offered={displayOffered}
            hideCycle
            hideModule
            display={display}
            onDisplayChange={setDisplay}
          />
        )
      }
      canAddView={canEdit}
      onOpenView={onOpenView}
      onAddView={() => {
        setCreateOpen(true);
      }}
      onEdit={() => {
        setEditing(true);
      }}
      onDelete={() => {
        setDeleting(true);
      }}
    />
  );
  const body = (
    <div className="min-h-0 flex-1">
      {bodyLayout === null ? (
        <div data-testid="workspace-view-empty-body" className="min-h-0 flex-1" />
      ) : (
        <React.Suspense fallback={<ListSkeleton />}>
          <GlobalIssuesScreen
            query={bodyQuery}
            hideHeader
            emptyState={
              <SavedViewEmptyCard
                canAct={canEdit}
                onCreate={() => {
                  setCreateWorkItemOpen(true);
                }}
              />
            }
            label={view.name}
          />
        </React.Suspense>
      )}
    </div>
  );
  const dialogs = (
    <>
      {createOpen ? (
        <WorkspaceViewDialog
          open={createOpen}
          onOpenChange={setCreateOpen}
          workspaceSlug={workspaceSlug}
          mode="create"
        />
      ) : null}
      {editing ? (
        <WorkspaceViewDialog
          open={editing}
          onOpenChange={setEditing}
          workspaceSlug={workspaceSlug}
          mode="edit"
          view={view}
        />
      ) : null}
      {deleting ? (
        <DeleteWorkspaceViewDialog
          open={deleting}
          onOpenChange={setDeleting}
          workspaceSlug={workspaceSlug}
          view={view}
          onDeleted={onOpenFallback}
        />
      ) : null}
      {createWorkItemOpen ? (
        <React.Suspense fallback={null}>
          <LazyWorkItemCreateDialog
            open={createWorkItemOpen}
            onOpenChange={setCreateWorkItemOpen}
            workspaceSlug={workspaceSlug}
            projects={creatable}
          />
        </React.Suspense>
      ) : null}
    </>
  );

  if (locked) {
    return (
      <section aria-label="Workspace view" className="flex min-h-0 flex-1 flex-col">
        {header(null)}
        <WorkspaceViewsTabStrip workspaceSlug={workspaceSlug} activeViewId={view.id} />
        {body}
        {dialogs}
      </section>
    );
  }
  return (
    <React.Suspense fallback={<WorkspaceDetailSkeleton />}>
      <LazyIssueFilters
        workspaceSlug={workspaceSlug}
        projectId=""
        entity={entity}
        hideCycle
        hideModule
        view={view}
        viewDisplay={display}
        onViewUpdated={adoptUpdatedView}
        onViewCreated={(created) => {
          adoptCreatedView(created);
          onOpenView(created.id);
        }}
      >
        {(parts) => (
          <section aria-label="Workspace view" className="flex min-h-0 flex-1 flex-col">
            {header(parts.control)}
            <WorkspaceViewsTabStrip workspaceSlug={workspaceSlug} activeViewId={view.id} />
            {parts.row}
            {body}
            {dialogs}
          </section>
        )}
      </LazyIssueFilters>
    </React.Suspense>
  );
}
