// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project view detail screen (parity/specs/views.md §7, VIEW-021–024, 028;
// §8, VIEW-025–027, 048). Reads one stored view and its project, then
// mounts the issues layout root with the view's own display settings as
// the source of the rendered layout, its stored expression as the scope
// of every page, and its stored order. The breadcrumb header carries the
// project → Views → view trail (last crumb a searchable switcher), the
// owner-only lock and the tab title; the layout toolbar carries the
// layout switcher, the Display control, the filter toggle and the Add
// entry. Layout and display switches edit a live copy and mark the view
// dirty; the filter row edits the stored expression; Update view persists
// either through the view's display-filter update path, and Save as forks
// the live snapshot through the create dialog. A locked view renders its
// stored layout with no switcher, no Display and no filter toggle. Any
// record that cannot be read — missing, refused, blank, failed — renders
// the missing-view state without layout chrome.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import type { Project, SavedView, SavedViewList } from "@pidash/api-client";
import { Button } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import {
  DisplayControl,
  IssuesLayoutRoot,
  LazyIssueFilters,
  ListPageQuickActions,
  ListSkeleton,
  SavedViewEmptyCard,
  ViewLayoutBody,
  issuesQueryOptions,
  peekSearchFor,
  stripPeekParams,
  mergeDisplayPrefs,
  parseDisplayPrefs,
  switchLayoutPrefs,
  selectCreatableProjects,
  useWeekStart,
  type IssueSearch,
  type LayoutKind,
  type PeekRequest,
  type RichFilterBuilderParts,
  type ViewSnapshot,
  type WorkItemCreateProject,
} from "../../issues/index.js";
import { useProjectRow } from "../../auth/index.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { hasEstimateLink } from "../../../shared/lib/estimateLink.js";
import { projectViewDetailQueryOptions } from "../api/detail.js";
import { viewKeys } from "../api/keys.js";
import { projectViewsQueryOptions } from "../api/queries.js";
import { replaceProjectViewRow } from "../lib/projectViewActions.js";
import { projectViewHref, projectViewsListHref, sortProjectViewsDefault } from "../lib/projectViews.js";
import { SavedViewHeader } from "./SavedViewHeader.js";
import { SavedViewMissingState } from "./SavedViewMissingState.js";
import { saveAsTitle, savedViewDocumentTitle } from "./model.js";
import type { SavedViewDetailParams } from "./preload.js";

const LazyWorkItemCreateDialog = React.lazy(() =>
  import("../../issues/components/WorkItemCreateDialog.js").then((module) => ({
    default: module.WorkItemCreateDialog,
  }))
);

// The Save-as duplicate opens the create dialog; it rides its own chunk
// with the filter section it hosts.
const LazySaveViewDialog = React.lazy(() =>
  import("../dialog/CreateViewDialog.js").then((module) => ({
    default: module.CreateViewDialog,
  }))
);

/** Every layout a view may store, in switcher order (VIEW-028). */
const VIEW_LAYOUTS: readonly LayoutKind[] = ["list", "kanban", "calendar", "spreadsheet", "gantt_chart"];

export interface SavedViewScreenProps {
  params: SavedViewDetailParams;
  /** Validated route search, exactly as the issues screen reads it. */
  search: IssueSearch;
}

export function SavedViewScreen({ params, search }: SavedViewScreenProps): React.ReactElement {
  const client = getAppClient();
  const navigate = useNavigate();
  const weekStart = useWeekStart();

  const projects = useQuery(projectsQueryOptions(client, params.ws));
  const project = projects.data?.find((entry) => entry.id === params.projectId) ?? null;
  // The estimate link lives on the project row alone; the workspace list
  // omits it, so a saved view reads the row for its sheet column (EST-018).
  const projectRow = useProjectRow(params.ws, params.projectId);
  const view = useQuery(projectViewDetailQueryOptions(client, params.ws, params.projectId, params.viewId));
  const views = useQuery(projectViewsQueryOptions(client, params.ws, params.projectId));

  // The tab title follows the record; while it loads or fails, the
  // project's name (or the area name) is still an honest title.
  useDocumentHead({
    title: savedViewDocumentTitle(project?.name, view.data ?? null),
    robots: "noindex, nofollow",
  });

  const openList = (): void => {
    void navigate({ to: projectViewsListHref(params.ws, params.projectId) });
  };

  // Every saved view lists all five layouts over one address, so the peek
  // binds the same way here as on the issues route: writes replace, never
  // push, and closing strips every peek param (ISS-167).
  const setPeek = (request: PeekRequest): void => {
    const next =
      request === null ? stripPeekParams({ ...search }) : peekSearchFor({ ...search }, request, params.projectId);
    void navigate({ to: ".", search: next, replace: true } as never);
  };

  if (view.isPending) {
    return (
      <section aria-label="Saved view" className="flex min-h-0 flex-1 flex-col p-(--space-4)">
        <ListSkeleton />
      </section>
    );
  }
  if (view.isError || view.data === null || view.data.id === "") {
    // VIEW-022: absent, refused and blank records all read the same to a
    // viewer who cannot see the record. No header chrome is rendered.
    return <SavedViewMissingState onBack={openList} />;
  }

  return (
    <SavedViewBody
      workspaceSlug={params.ws}
      projectId={params.projectId}
      projectName={project?.name ?? null}
      projectIdentifier={project?.identifier ?? null}
      memberRole={project?.member_role ?? null}
      projectArchived={project?.archived_at != null}
      cycleView={project?.cycle_view === true}
      moduleView={project?.module_view === true}
      estimateEnabled={hasEstimateLink(projectRow.data)}
      viewsEnabled={project?.issue_views_view === true}
      search={search}
      weekStart={weekStart}
      view={view.data}
      projects={projects.data ?? []}
      views={sortProjectViewsDefault(views.data ?? [])}
      viewsLoading={views.isPending}
      viewsFailed={views.isError}
      onPeekChange={setPeek}
      onOpenView={(viewId) => {
        void navigate({ to: projectViewHref(params.ws, params.projectId, viewId) });
      }}
      onOpenProject={() => {
        void navigate({ to: `/${params.ws}/projects/${params.projectId}` });
      }}
    />
  );
}

interface SavedViewBodyProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string | null;
  projectIdentifier: string | null;
  memberRole: number | null;
  projectArchived: boolean;
  cycleView: boolean;
  moduleView: boolean;
  estimateEnabled: boolean;
  /** The project's views feature; the row's saves wait for it. */
  viewsEnabled: boolean;
  search: IssueSearch;
  weekStart: number;
  view: SavedView;
  projects: readonly Project[];
  views: readonly SavedView[];
  viewsLoading: boolean;
  viewsFailed: boolean;
  onPeekChange: (request: PeekRequest) => void;
  onOpenView: (viewId: string) => void;
  onOpenProject: () => void;
}

function SavedViewBody({
  workspaceSlug,
  projectId,
  projectName,
  projectIdentifier,
  memberRole,
  projectArchived,
  cycleView,
  moduleView,
  estimateEnabled,
  viewsEnabled,
  search,
  weekStart,
  view,
  projects,
  views,
  viewsLoading,
  viewsFailed,
  onPeekChange,
  onOpenView,
  onOpenProject,
}: SavedViewBodyProps): React.ReactElement {
  const client = getAppClient();
  const queryClient = useQueryClient();
  const projectEntity = React.useMemo(() => ({ scope: "project" as const, projectId }), [projectId]);
  const locked = view.is_locked;
  // The stored record decides the layout; the toolbar's switches move a
  // live copy, which marks the view dirty until Update view persists it.
  const storedDisplay = React.useMemo(
    () => parseDisplayPrefs(view.display_filters, view.display_properties),
    [view.display_filters, view.display_properties]
  );
  const [display, setDisplay] = React.useState(storedDisplay);
  React.useEffect(() => {
    setDisplay(storedDisplay);
  }, [storedDisplay]);

  const issues = useQuery(
    issuesQueryOptions(client, workspaceSlug, projectId, search, view.rich_filters, display.orderBy)
  );
  const [createOpen, setCreateOpen] = React.useState(false);
  // The Save-as snapshot the row hands over; the dialog stays mounted
  // while one is held.
  const [saveAs, setSaveAs] = React.useState<ViewSnapshot | null>(null);
  // The empty card's create entry uses the same creatable-project rule as
  // the issues list: a guest on this project sees it dead.
  const creatable = React.useMemo<WorkItemCreateProject[]>(
    () =>
      selectCreatableProjects(
        projects.map((entry) => ({
          id: entry.id,
          name: entry.name,
          identifier: entry.identifier,
          memberRole: entry.member_role,
          archived: entry.archived_at !== null,
        }))
      ),
    [projects]
  );

  const canEdit = memberRole === null ? false : canEditInProject({ memberRole, archived: projectArchived });
  const rows = issues.data?.results ?? [];
  const totalCount = issues.data?.total_count ?? 0;

  const emptyCard = (
    <SavedViewEmptyCard
      canAct={canEdit}
      onCreate={() => {
        setCreateOpen(true);
      }}
    />
  );
  // The view's own settings, not the personal record: Display edits the
  // live copy and Update view persists it.
  const displayControl = (
    <DisplayControl
      workspaceSlug={workspaceSlug}
      entity={projectEntity}
      offered={VIEW_LAYOUTS}
      hideCycle={!cycleView}
      hideModule={!moduleView}
      display={display}
      onDisplayChange={setDisplay}
    />
  );
  // Project members and admins only; guests never see the entry. The modal
  // opens scoped to this project and never touches the view record.
  const addWorkItem = canEdit ? (
    <Button
      variant="primary"
      size="small"
      onClick={() => {
        setCreateOpen(true);
      }}
    >
      Add work item
    </Button>
  ) : null;

  /** Adopt an Update-view PATCH echo: the record, the list row, the live copy. */
  function adoptUpdatedView(stored: SavedView): void {
    queryClient.setQueryData(viewKeys.projectDetail(workspaceSlug, projectId, view.id), stored);
    queryClient.setQueryData(viewKeys.projectList(workspaceSlug, projectId), (list: SavedViewList | undefined) =>
      replaceProjectViewRow(list, stored)
    );
    setDisplay(parseDisplayPrefs(stored.display_filters, stored.display_properties));
  }

  const renderWithFilters = (richFilter: RichFilterBuilderParts | null): React.ReactElement => (
    <IssuesLayoutRoot
      workspaceSlug={workspaceSlug}
      entity={projectEntity}
      title={view.name}
      totalCount={totalCount}
      resultCount={rows.length}
      offered={VIEW_LAYOUTS}
      display={display}
      onDisplayChange={(next) => {
        setDisplay((current) => mergeDisplayPrefs(current, switchLayoutPrefs(current, next)));
      }}
      renderLayout={(active, prefs) => (
        <ViewLayoutBody
          layout={active}
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          projectIdentifier={projectIdentifier ?? ""}
          entity={projectEntity}
          prefs={prefs}
          canEdit={canEdit}
          richExpression={view.rich_filters}
          hideCycle={!cycleView}
          hideModule={!moduleView}
          hideEstimate={!estimateEnabled}
          weekStart={weekStart}
          issues={rows}
          totalCount={totalCount}
          compact={search.layout === "compact"}
          peek={{
            peekKey: search.peekIssueId ?? null,
            peekProjectId: search.peekProjectId ?? null,
            onPeekChange,
          }}
        />
      )}
      emptyState={emptyCard}
      noMatchState={emptyCard}
      hideSwitcher={locked}
      headerActions={
        <>
          {addWorkItem}
          {locked ? null : displayControl}
          <ListPageQuickActions />
        </>
      }
      displayControl={locked ? undefined : displayControl}
      filterControl={locked ? undefined : richFilter?.control}
      filterRow={locked ? undefined : richFilter?.row}
    />
  );

  return (
    <section aria-label="Saved view" className="flex min-h-0 flex-1 flex-col">
      <SavedViewHeader
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={projectName ?? ""}
        view={view}
        views={views}
        viewsLoading={viewsLoading}
        viewsFailed={viewsFailed}
        onOpenProject={onOpenProject}
        onOpenView={onOpenView}
      />
      {issues.isPending ? (
        <div className="min-h-0 flex-1 p-(--space-4)">
          <ListSkeleton />
        </div>
      ) : (
        <React.Suspense fallback={renderWithFilters(null)}>
          <LazyIssueFilters
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            entity={projectEntity}
            hideCycle={!cycleView}
            hideModule={!moduleView}
            viewsEnabled={viewsEnabled}
            view={view}
            viewDisplay={display}
            onSavePress={(snapshot) => {
              setSaveAs(snapshot);
            }}
            onViewUpdated={adoptUpdatedView}
          >
            {renderWithFilters}
          </LazyIssueFilters>
        </React.Suspense>
      )}
      {createOpen ? (
        <React.Suspense fallback={null}>
          <LazyWorkItemCreateDialog
            open={createOpen}
            onOpenChange={setCreateOpen}
            workspaceSlug={workspaceSlug}
            projects={creatable}
            initialProjectId={projectId}
          />
        </React.Suspense>
      ) : null}
      {saveAs !== null ? (
        <React.Suspense fallback={null}>
          <LazySaveViewDialog
            open
            onOpenChange={() => {
              setSaveAs(null);
            }}
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            hideCycle={!cycleView}
            hideModule={!moduleView}
            prefill={{ title: saveAsTitle(view.name), display, expression: saveAs.expression }}
          />
        </React.Suspense>
      ) : null}
    </section>
  );
}
