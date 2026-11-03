// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Data-connected projects list (SHELL-024, SHELL-025, SHELL-028
// through SHELL-032, SHELL-037 through SHELL-042). Subscribes to the
// shared workspace collection, the membership options and the
// caller's workspace role, narrows and orders the rows through the
// display store, and mounts the sort, filter, pills and search
// controls into the screen slots. Each card renders through the card
// renderer, which feeds it the member directory, the star
// entitlement, the favorite toggle and the lifecycle dialog openers;
// joins and lifecycle completions land through client-side
// navigation. Creation mounts into the screen's dialog slot. The
// routes stay thin: they resolve the workspace slug and mount this
// view.
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";
import type { ProjectDetails } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { useMe, usePermissions } from "../../../core/session/index.js";
import { workspaceMembersQueryOptions } from "../api/members.js";
import { useToggleFavoriteProject } from "../api/mutations.js";
import { projectDetailsQueryOptions } from "../api/queries.js";
import {
  DEFAULT_LIST_SORT,
  EMPTY_LIST_FILTERS,
  filterListRows,
  hasNarrowingListFilters,
  sortListRows,
} from "../lib/list-display.js";
import { registerListDisplayReset, useListDisplayStore } from "../store/list-display.js";
import { CreateProjectDialog } from "./CreateProjectDialog.js";
import { DeleteProjectDialog } from "./DeleteProjectDialog.js";
import { ProjectArchiveDialog, type ArchiveDialogMode } from "./ProjectArchiveDialog.js";
import { ProjectCard } from "./ProjectCard.js";
import { ProjectsAppliedFilters } from "./ProjectsAppliedFilters.js";
import { ProjectsFilterPanel } from "./ProjectsFilterPanel.js";
import { ProjectsListScreen, selectViewRows, type CreateDialogControls } from "./ProjectsListScreen.js";
import { ProjectsListSearch } from "./ProjectsListSearch.js";
import { ProjectsSortMenu } from "./ProjectsSortMenu.js";

export interface ProjectsListViewProps {
  workspaceSlug: string;
  /** Archive view: archived rows, the Archived crumb, no creation. */
  archived?: boolean;
}

interface LifecycleSelection {
  mode: ArchiveDialogMode | "delete";
  project: ProjectDetails;
}

export function ProjectsListView({ workspaceSlug, archived = false }: ProjectsListViewProps): React.ReactElement {
  const client = getAppClient();
  const details = useQuery(projectDetailsQueryOptions(client, workspaceSlug));
  const memberships = useQuery(workspaceMembersQueryOptions(client, workspaceSlug));
  const { canEdit } = usePermissions(workspaceSlug);
  const me = useMe();
  const sort = useListDisplayStore((state) => state.displays[workspaceSlug]?.sort ?? DEFAULT_LIST_SORT);
  const filters = useListDisplayStore((state) => state.displays[workspaceSlug]?.filters ?? EMPTY_LIST_FILTERS);
  const searchQuery = useListDisplayStore((state) => state.searchQuery);
  const setSort = useListDisplayStore((state) => state.setSort);
  const setMyProjects = useListDisplayStore((state) => state.setMyProjects);
  const toggleAccess = useListDisplayStore((state) => state.toggleAccess);
  const togglePerson = useListDisplayStore((state) => state.togglePerson);
  const selectCreated = useListDisplayStore((state) => state.selectCreated);
  const removeFilterValue = useListDisplayStore((state) => state.removeFilterValue);
  const removeFilterDimension = useListDisplayStore((state) => state.removeFilterDimension);
  const clearListFilters = useListDisplayStore((state) => state.clearListFilters);
  const setSearchQuery = useListDisplayStore((state) => state.setSearchQuery);
  const navigate = useNavigate();
  const favorite = useToggleFavoriteProject();
  const [lifecycle, setLifecycle] = React.useState<LifecycleSelection | null>(null);

  // The display store joins the sign-out reset list here (not at its own
  // module scope) so the store stays side-effect-free: the initial bundle
  // drops it when only lazy routes render this view (NEWFRONT-523). The
  // registry dedupes repeat visits; the state is in-memory, so a sign-out
  // before the first visit has nothing to clear.
  React.useEffect(() => {
    registerListDisplayReset();
  }, []);

  React.useEffect(() => {
    useListDisplayStore.getState().trackWorkspace(workspaceSlug);
  }, [workspaceSlug]);

  const rows = React.useMemo(() => {
    if (details.data === undefined) return undefined;
    return sortListRows(filterListRows(selectViewRows(details.data, archived), filters, searchQuery), sort);
  }, [details.data, archived, filters, searchQuery, sort]);

  const handleNavigate = React.useCallback((href: string) => void navigate({ to: href }), [navigate]);
  const closeLifecycle = React.useCallback(() => setLifecycle(null), []);
  const renderCard = React.useCallback(
    (project: ProjectDetails) => (
      <ProjectCard
        workspaceSlug={workspaceSlug}
        project={project}
        members={memberships.data}
        canStar={canEdit}
        starPending={favorite.pendingProjectId === project.id}
        onToggleFavorite={(row) => favorite.mutate({ workspaceSlug, project: row })}
        onArchive={(row) => setLifecycle({ mode: "archive", project: row })}
        onRestore={(row) => setLifecycle({ mode: "restore", project: row })}
        onDelete={(row) => setLifecycle({ mode: "delete", project: row })}
        onNavigate={handleNavigate}
      />
    ),
    [workspaceSlug, memberships.data, canEdit, favorite.mutate, favorite.pendingProjectId, handleNavigate]
  );
  const renderCreateDialog = React.useCallback(
    (controls: CreateDialogControls) => (
      <CreateProjectDialog
        workspaceSlug={workspaceSlug}
        members={memberships.data}
        open={controls.open}
        onOpenChange={controls.onOpenChange}
      />
    ),
    [workspaceSlug, memberships.data]
  );
  return (
    <>
      <ProjectsListScreen
        workspaceSlug={workspaceSlug}
        archived={archived}
        collection={details.data}
        canCreate={canEdit}
        renderCard={renderCard}
        renderCreateDialog={renderCreateDialog}
        searchQuery={searchQuery}
        {...(rows === undefined ? {} : { projects: rows })}
        hasActiveFilters={hasNarrowingListFilters(filters)}
        searchControl={<ProjectsListSearch query={searchQuery} onQueryChange={setSearchQuery} />}
        sortControl={<ProjectsSortMenu sort={sort} onSortChange={(next) => setSort(workspaceSlug, next)} />}
        filterControl={
          <ProjectsFilterPanel
            filters={filters}
            members={memberships.data}
            selfId={me.data?.id ?? null}
            onToggleMyProjects={() => setMyProjects(workspaceSlug, !filters.myProjects)}
            onToggleAccess={(value) => toggleAccess(workspaceSlug, value)}
            onTogglePerson={(dimension, id) => togglePerson(workspaceSlug, dimension, id)}
            onSelectCreated={(values) => selectCreated(workspaceSlug, values)}
          />
        }
        filterPills={
          <ProjectsAppliedFilters
            filters={filters}
            members={memberships.data ?? []}
            filteredCount={rows?.length ?? 0}
            totalCount={details.data?.length ?? 0}
            onRemoveMyProjects={() => setMyProjects(workspaceSlug, false)}
            onRemoveValue={(dimension, value) => removeFilterValue(workspaceSlug, dimension, value)}
            onRemoveDimension={(dimension) => removeFilterDimension(workspaceSlug, dimension)}
            onClearAll={() => clearListFilters(workspaceSlug)}
          />
        }
      />
      {lifecycle === null ? null : lifecycle.mode === "delete" ? (
        <DeleteProjectDialog
          workspaceSlug={workspaceSlug}
          projectId={lifecycle.project.id}
          projectName={lifecycle.project.name}
          open
          onOpenChange={closeLifecycle}
        />
      ) : (
        <ProjectArchiveDialog
          workspaceSlug={workspaceSlug}
          projectId={lifecycle.project.id}
          projectName={lifecycle.project.name}
          mode={lifecycle.mode}
          open
          onOpenChange={closeLifecycle}
          onNavigate={handleNavigate}
        />
      )}
    </>
  );
}
