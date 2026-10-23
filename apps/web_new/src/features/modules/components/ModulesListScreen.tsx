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
// slices (the timeline's manual reorder).

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { meQueryOptions } from "../../../core/session/queries.js";
import { useDocumentHead, useNarrowViewport } from "../../../shared/shell/index.js";
import { useProjectRoles } from "../../auth/index.js";
import { isProjectAdmin, workspaceMembersQueryOptions } from "../../projects/index.js";
import { moduleProjectFactsQueryOptions, moduleViewEnabled } from "../api/projectFacts.js";
import { modulesListQueryOptions } from "../api/queries.js";
import { MODULES_CRUMB, modulesFeatureHref, modulesTitle, resolveModulesListBody } from "../lib/moduleList.js";
import { resolveCompactLayout } from "../lib/moduleLayouts.js";
import { useModuleLayout } from "../lib/listLayoutStore.js";
import { hasModuleFilters } from "../lib/moduleListControls.js";
import { useModuleListDisplay } from "../lib/moduleListDisplayStore.js";
import { ModuleListProvider } from "./moduleListContext.js";
import { ModulesFeatureGate } from "./ModulesFeatureGate.js";
import { ModulesFilterChips } from "./ModulesFilterChips.js";
import { ModulesFilterMenu } from "./ModulesFilterMenu.js";
import { ModulesListBody } from "./ModulesListBody.js";
import { ModulesListHeader } from "./ModulesListHeader.js";
import { ModulesListSkeleton } from "./ModulesListSkeleton.js";
import { ModulesSearch } from "./ModulesSearch.js";
import { ModulesSortMenu } from "./ModulesSortMenu.js";

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

  return (
    <ModuleListProvider value={context}>
      <section
        aria-label={MODULES_CRUMB}
        data-testid="modules-list-screen"
        className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-4)"
      >
        {state === "gate" || project.data === undefined ? null : (
          <ModulesListHeader
            projectName={project.data.name}
            layout={layout}
            onLayoutChange={setLayout}
            searchControl={<ModulesSearch query={display.query} onQueryChange={setQuery} />}
            orderControl={<ModulesSortMenu sort={display.sort} onSortChange={setSort} />}
            filterControl={
              <ModulesFilterMenu
                filters={display.filters}
                members={directory.data}
                meId={me.data?.id ?? null}
                onFiltersChange={setFilters}
              />
            }
            chips={
              <ModulesFilterChips
                filters={display.filters}
                members={directory.data}
                meId={me.data?.id ?? null}
                onFiltersChange={setFilters}
              />
            }
          />
        )}
        {state === "gate" ? (
          <ModulesFeatureGate
            isAdmin={isProjectAdmin(roles.data?.[projectId] ?? null)}
            onManageFeatures={() => {
              void navigate({ to: modulesFeatureHref(workspaceSlug, projectId) });
            }}
          />
        ) : state === "skeleton" ? (
          <ModulesListSkeleton layout={shownLayout} />
        ) : (
          (children ?? (
            <ModulesListBody
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              layout={shownLayout}
              modules={modules.data ?? []}
            />
          ))
        )}
      </section>
    </ModuleListProvider>
  );
}
