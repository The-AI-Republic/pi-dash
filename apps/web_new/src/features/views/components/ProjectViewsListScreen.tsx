// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views list screen (parity/specs/views.md §3-4, VIEW-001/002 and
// VIEW-004-007). Reads the project row for the feature gate, the saved-views
// collection for the list, and the roster for owner and creator chips; sets
// the tab title; and composes the header with exactly one body state. The
// name search, order and filters are visit-local state: the controls own
// them here, nothing is persisted, and the fetched records are never
// modified. No list fetch runs while the gate shows.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useMe, useWorkspace } from "../../../core/session/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { useProjectRoles, useProjectRow } from "../../auth/index.js";
import { preloadRichFilterBuilder } from "../../issues/index.js";
import { isProjectAdmin, workspaceMembersQueryOptions } from "../../projects/index.js";
import { projectViewsQueryOptions } from "../api/queries.js";
import { CreateViewDialog } from "../dialog/CreateViewDialog.js";
import {
  PROJECT_VIEWS_CRUMB,
  projectViewsFeatureHref,
  projectViewsTitle,
  resolveProjectViewsBody,
} from "../lib/projectViews.js";
import {
  DEFAULT_VIEW_SORT,
  EMPTY_VIEW_LIST_FILTERS,
  filterProjectViews,
  sortProjectViews,
  type ViewListFilters,
  type ViewSort,
} from "../lib/viewListControls.js";
import { ProjectViewsAppliedFilters } from "./ProjectViewsAppliedFilters.js";
import { ProjectViewsFilterMenu } from "./ProjectViewsFilterMenu.js";
import { ProjectViewsGate } from "./ProjectViewsGate.js";
import { ProjectViewsError } from "./ProjectViewsError.js";
import { ProjectViewsListBody } from "./ProjectViewsListBody.js";
import { ProjectViewsListHeader } from "./ProjectViewsListHeader.js";
import { ProjectViewsSearch } from "./ProjectViewsSearch.js";
import { ProjectViewsSortMenu } from "./ProjectViewsSortMenu.js";
import { ProjectViewsSkeleton } from "./ProjectViewsSkeleton.js";

export interface ProjectViewsListScreenProps {
  workspaceSlug: string;
  projectId: string;
  /**
   * Opens the create dialog. Slot owner: NEWFRONT-721. An explicit
   * opener overrides the default, which opens the screen's own dialog.
   */
  onCreateView?: (() => void) | undefined;
}

export function ProjectViewsListScreen({
  workspaceSlug,
  projectId,
  onCreateView,
}: ProjectViewsListScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const project = useProjectRow(workspaceSlug, projectId);
  const gateOff = project.data?.issue_views_view === false;
  const views = useQuery({
    ...projectViewsQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: project.isSuccess && !gateOff,
  });
  const roles = useProjectRoles(workspaceSlug);
  const members = useQuery(workspaceMembersQueryOptions(getAppClient(), workspaceSlug));
  const { workspace } = useWorkspace(workspaceSlug);
  const me = useMe();
  const [createOpen, setCreateOpen] = React.useState(false);
  // Visit-local controls (VIEW-004-007): a reload starts from the defaults.
  const [searchQuery, setSearchQuery] = React.useState("");
  const [sort, setSort] = React.useState<ViewSort>(DEFAULT_VIEW_SORT);
  const [filters, setFilters] = React.useState<ViewListFilters>(EMPTY_VIEW_LIST_FILTERS);
  // Start the dialog's filter chunk while the list data loads, so opening
  // the dialog rarely flashes its loading note (mirrors NEWFRONT-619).
  React.useEffect(() => {
    preloadRichFilterBuilder();
  }, []);

  useDocumentHead({
    title: project.data === undefined ? PROJECT_VIEWS_CRUMB : projectViewsTitle(project.data.name),
    robots: "noindex, nofollow",
  });

  if (project.isPending) {
    return (
      <section
        aria-label={PROJECT_VIEWS_CRUMB}
        data-testid="project-views-screen"
        className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
      >
        <ProjectViewsSkeleton />
      </section>
    );
  }
  if (project.isError) {
    return (
      <section
        aria-label={PROJECT_VIEWS_CRUMB}
        data-testid="project-views-screen"
        className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
      >
        <ProjectViewsError
          onRetry={() => {
            void project.refetch();
          }}
        />
      </section>
    );
  }
  if (gateOff) {
    return (
      <section
        aria-label={PROJECT_VIEWS_CRUMB}
        data-testid="project-views-screen"
        className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
      >
        <ProjectViewsGate
          isAdmin={isProjectAdmin(roles.data?.[projectId] ?? null)}
          onManageFeatures={() => {
            void navigate({ to: projectViewsFeatureHref(workspaceSlug, projectId) });
          }}
        />
      </section>
    );
  }

  const fetched = views.data ?? [];
  const visible = sortProjectViews(filterProjectViews(fetched, filters, searchQuery, Date.now()), sort);
  const state = resolveProjectViewsBody({
    viewsPending: views.isPending,
    viewsError: views.isError,
    viewCount: fetched.length,
    visibleCount: visible.length,
  });
  // The header and the zero state share one opener (VIEW-003/009): the
  // explicit prop when a host overrides it, else the screen's own dialog.
  const openCreateView = onCreateView ?? (() => setCreateOpen(true));
  const selfId = me.data?.id ?? null;
  return (
    <section
      aria-label={PROJECT_VIEWS_CRUMB}
      data-testid="project-views-screen"
      className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
    >
      <ProjectViewsListHeader
        workspaceName={workspace?.name ?? workspaceSlug}
        projectName={project.data.name}
        onCreateView={openCreateView}
        searchControl={<ProjectViewsSearch query={searchQuery} onQueryChange={setSearchQuery} />}
        sortControl={<ProjectViewsSortMenu sort={sort} onSortChange={setSort} />}
        filterControl={
          <ProjectViewsFilterMenu
            filters={filters}
            members={members.data}
            selfId={selfId}
            onFiltersChange={setFilters}
          />
        }
      />
      <ProjectViewsAppliedFilters
        filters={filters}
        members={members.data}
        selfId={selfId}
        onFiltersChange={setFilters}
      />
      <ProjectViewsListBody
        state={state}
        views={visible}
        members={members.data}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        viewerId={me.data?.id ?? null}
        projectRole={roles.data?.[projectId] ?? null}
        hideCycle={!project.data.cycle_view}
        hideModule={!project.data.module_view}
        onCreateView={openCreateView}
        onRetry={() => {
          void views.refetch();
        }}
      />
      <CreateViewDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        hideCycle={!project.data.cycle_view}
        hideModule={!project.data.module_view}
      />
    </section>
  );
}
