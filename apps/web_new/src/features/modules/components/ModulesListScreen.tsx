// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project modules list (parity/specs/modules.md §2.1–§2.7 and §3.1–§3.3,
// MOD-001–011). Reads the project facts for the name, the feature switch
// and the zone, the modules collection for the body and the role map for
// the gate's shortcut; sets the tab title; and composes exactly one body
// state — gate, skeleton or content. The facts read is the modules
// feature's own tolerant projection: a project record without
// `module_view` parses and reads as off, so it gates instead of leaving
// the skeleton up (MOD-002). No list read runs while the gate shows, and
// the gate never flashes a skeleton first. The layout, search, ordering
// and filters are the viewer's per-project browser record (§13.3.1). This
// slice owns the header controls, the applied-chip strip and the
// no-match container; the list context exposes the chosen sort to later
// slices (the timeline's manual reorder). Slice F adds the two creation
// entry points — the header action and the zero-state shortcut — and the
// dialog provider whose openers every later menu calls. An empty project
// renders the onboarding state instead of an empty list. The create action
// is mounted only for admin and member standing, so guests never see it.

import type { ModuleRow, WorkspaceMember } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { useDocumentHead, useNarrowViewport } from "../../../shared/shell/index.js";
import { useProjectRoles } from "../../auth/index.js";
import { isProjectAdmin, isProjectAuthorized, workspaceMembersQueryOptions } from "../../projects/index.js";
import { moduleProjectFactsQueryOptions, moduleProjectIdentifier, moduleViewEnabled } from "../api/projectFacts.js";
import { modulesListQueryOptions } from "../api/queries.js";
import { MODULES_CRUMB, modulesFeatureHref, modulesTitle, resolveModulesListBody } from "../lib/moduleList.js";
import { resolveCompactLayout, type ModulesLayout } from "../lib/moduleLayouts.js";
import { useModuleLayout } from "../lib/listLayoutStore.js";
import { hasModuleFilters, type ModuleListFilters, type ModuleSort } from "../lib/moduleListControls.js";
import { useModuleListDisplay, type ModuleListDisplay } from "../lib/moduleListDisplayStore.js";
import { ModuleCreateAction } from "./dialogs/ModuleCreateAction.js";
import { ModuleDialogProvider, useModuleDialogs } from "./dialogs/ModuleDialogProvider.js";
import { ModuleListProvider } from "./moduleListContext.js";
import { ModulesFeatureGate } from "./ModulesFeatureGate.js";
import { ModulesFilterChips } from "./ModulesFilterChips.js";
import { ModulesFilterMenu } from "./ModulesFilterMenu.js";
import { ModulesListBody } from "./ModulesListBody.js";
import { ModulesListHeader } from "./ModulesListHeader.js";
import { ModulesListSkeleton } from "./ModulesListSkeleton.js";
import { ModulesSearch } from "./ModulesSearch.js";
import { ModulesSortMenu } from "./ModulesSortMenu.js";
import { ModulesZeroState } from "./ModulesZeroState.js";

/** The screen's one page slot: header (or nothing) over the body state. */
function listSection(header: React.ReactNode, body: React.ReactNode): React.ReactElement {
  return (
    <section
      aria-label={MODULES_CRUMB}
      data-testid="modules-list-screen"
      className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
    >
      {header}
      {body}
    </section>
  );
}

/**
 * The settled list surface: header controls plus the body the screen chose.
 * It is a stable top-level component on purpose — opening the filters and
 * picking a value re-renders the screen, and an inline component type
 * would remount the panel (closing it) on every selection.
 */
interface ModulesListSettledProps {
  workspaceSlug: string;
  projectId: string;
  projectName: string;
  layout: ModulesLayout;
  onLayoutChange: (layout: ModulesLayout) => void;
  narrow: boolean;
  createAllowed: boolean;
  display: ModuleListDisplay;
  onQueryChange: (query: string) => void;
  onSortChange: (sort: ModuleSort) => void;
  onFiltersChange: (filters: ModuleListFilters) => void;
  members: readonly WorkspaceMember[] | undefined;
  meId: string | null;
  shownLayout: ModulesLayout;
  loading: boolean;
  modules: readonly ModuleRow[];
  /** The list read settled empty — onboarding replaces the empty body. */
  empty: boolean;
  children?: React.ReactNode;
}

function ModulesListSettled({
  workspaceSlug,
  projectId,
  projectName,
  layout,
  onLayoutChange,
  narrow,
  createAllowed,
  display,
  onQueryChange,
  onSortChange,
  onFiltersChange,
  members,
  meId,
  shownLayout,
  loading,
  modules,
  empty,
  children,
}: ModulesListSettledProps): React.ReactElement {
  const dialogs = useModuleDialogs();
  const openCreate = (): void => {
    dialogs?.openCreate();
  };
  return listSection(
    <ModulesListHeader
      projectName={projectName}
      layout={layout}
      onLayoutChange={onLayoutChange}
      createControl={createAllowed ? <ModuleCreateAction compact={narrow} onCreate={openCreate} /> : null}
      searchControl={<ModulesSearch query={display.query} onQueryChange={onQueryChange} />}
      orderControl={<ModulesSortMenu sort={display.sort} onSortChange={onSortChange} />}
      filterControl={
        <ModulesFilterMenu filters={display.filters} members={members} meId={meId} onFiltersChange={onFiltersChange} />
      }
      chips={
        <ModulesFilterChips filters={display.filters} members={members} meId={meId} onFiltersChange={onFiltersChange} />
      }
    />,
    children !== undefined ? (
      children
    ) : loading ? (
      <ModulesListSkeleton layout={shownLayout} />
    ) : empty ? (
      // Onboarding is for a project that truly has no modules; a failed
      // read is not proof of that, so it keeps the plain list shell.
      <ModulesZeroState canCreate={createAllowed} onCreate={openCreate} />
    ) : (
      <ModulesListBody workspaceSlug={workspaceSlug} projectId={projectId} layout={shownLayout} modules={modules} />
    )
  );
}

export interface ModulesListScreenProps {
  workspaceSlug: string;
  projectId: string;
  /**
   * The list body. Absent, the screen renders its own rows, cards or
   * timeline rows; slices B and F wrap this set with narrowing and the
   * empty containers.
   */
  children?: React.ReactNode;
}

export function ModulesListScreen({ workspaceSlug, projectId, children }: ModulesListScreenProps): React.ReactElement {
  const navigate = useNavigate();
  const narrow = useNarrowViewport();
  const project = useQuery(moduleProjectFactsQueryOptions(getAppClient(), workspaceSlug, projectId));
  // Only a literal `true` opens the list; an absent flag is off (§2.2).
  const gateOff = project.data !== undefined && !moduleViewEnabled(project.data.module_view);
  const modules = useQuery({
    ...modulesListQueryOptions(getAppClient(), workspaceSlug, projectId),
    enabled: project.isSuccess && !gateOff,
  });
  const roles = useProjectRoles(workspaceSlug);
  const { layout, setLayout } = useModuleLayout(projectId);
  const { display, setQuery, setSort, setFilters } = useModuleListDisplay(projectId);
  // The lead/member options are the workspace directory; the current user
  // is offered first as "You".
  const directory = useQuery(workspaceMembersQueryOptions(getAppClient(), workspaceSlug));
  const me = useQuery(meQueryOptions(getAppClient()));
  const filtersActive = hasModuleFilters(display.filters);

  useDocumentHead({
    title: project.data === undefined ? MODULES_CRUMB : modulesTitle(project.data.name),
    robots: "noindex, nofollow",
  });

  const state = resolveModulesListBody({
    projectPending: project.isPending,
    gateOff,
    modulesPending: modules.isPending,
  });
  // Small screens never render the timeline from the list; the stored
  // choice survives for the next wide visit.
  const shownLayout = narrow ? resolveCompactLayout(layout) : layout;

  const context = {
    query: display.query,
    setQuery,
    sort: display.sort,
    setSort,
    filters: display.filters,
    setFilters,
    filtersActive,
  };
  const createAllowed = isProjectAuthorized(roles.data?.[projectId] ?? null);
  const isAdmin = isProjectAdmin(roles.data?.[projectId] ?? null);

  // Before the project resolves there is nothing to gate on: the
  // skeleton stands alone, with no header and no reads behind it.
  if (project.data === undefined) {
    return (
      <ModuleListProvider value={context}>
        {listSection(null, <ModulesListSkeleton layout={shownLayout} />)}
      </ModuleListProvider>
    );
  }
  if (state === "gate") {
    return (
      <ModuleListProvider value={context}>
        {listSection(
          null,
          <ModulesFeatureGate
            isAdmin={isAdmin}
            onManageFeatures={() => {
              void navigate({ to: modulesFeatureHref(workspaceSlug, projectId) });
            }}
          />
        )}
      </ModuleListProvider>
    );
  }

  return (
    <ModuleDialogProvider
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      currentProject={{
        id: projectId,
        name: project.data.name,
        identifier: moduleProjectIdentifier(project.data),
      }}
    >
      <ModuleListProvider value={context}>
        <ModulesListSettled
          workspaceSlug={workspaceSlug}
          projectId={projectId}
          projectName={project.data.name}
          layout={layout}
          onLayoutChange={setLayout}
          narrow={narrow}
          createAllowed={createAllowed}
          display={display}
          onQueryChange={setQuery}
          onSortChange={setSort}
          onFiltersChange={setFilters}
          members={directory.data}
          meId={me.data?.id ?? null}
          shownLayout={shownLayout}
          loading={state === "skeleton"}
          modules={modules.data ?? []}
          empty={modules.isSuccess && modules.data.length === 0}
        >
          {children}
        </ModulesListSettled>
      </ModuleListProvider>
    </ModuleDialogProvider>
  );
}
