// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail screen (parity/specs/modules.md §8, MOD-029, MOD-030,
// MOD-032). The modules-owned frame around the shared entity host:
// the breadcrumb trail with the module switcher, the work-item count
// badge, the sidebar toggle and the module actions menu in the desktop
// header, the tab title, and the analytics sidebar beside the hosted
// layouts. The gone-away gate wraps this screen at the route; guests
// render the same frame with the host and sidebar refusing their edits,
// and the actions menu offering only the read-only link entries (§4.5).

import { useSuspenseQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { canEditInProject } from "../../../shared/shell/contextual/index.js";
import { isCompletedModule } from "../../../shared/pickers/modules.js";
import { EntityListScreen, listPageQueryOptions, prefsQueryOptions, selectDisplayPrefs } from "../../issues/index.js";
import type { PeekRequest } from "../../issues/index.js";
import { moduleDetailTitle } from "../lib/moduleDetail.js";
import { toModuleCreateProject } from "../lib/moduleAddWorkItem.js";
import { useModuleAddWorkItem } from "../lib/useModuleAddWorkItem.js";
import { ModuleAddWorkItem } from "./ModuleAddWorkItem.js";
import { MODULES_CRUMB } from "../lib/moduleList.js";
import { useDetailSidebarCollapsed } from "../lib/detailSidebarStore.js";
import { ModuleAnalyticsSidebar } from "./sidebar/ModuleAnalyticsSidebar.js";
import { ModuleProgressSection } from "./sidebar/ModuleProgressSection.js";
import { moduleDetailQueryOptions } from "./sidebar/moduleSidebarQueries.js";
import { ModuleDetailBadge } from "./ModuleDetailBadge.js";
import { ModuleDetailPaletteEntries } from "./ModuleDetailPaletteEntries.js";
import { ModuleDetailActions } from "./dialogs/ModuleDetailActions.js";
import { moduleDetailMenuEntries } from "./menus/moduleDetailMenuEntries.js";
import { ModuleDetailCrumbs } from "./ModuleDetailCrumbs.js";
import { ModuleSidebarToggle } from "./ModuleSidebarToggle.js";
export interface ModuleDetailScreenProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  peekKey: string | null;
  onPeekChange: (request: PeekRequest) => void;
  /** Peeked project from the URL, when it differs from the route (ISS-167). */
  peekProjectId?: string | null | undefined;
  compact: boolean;
}

export function ModuleDetailScreen({
  workspaceSlug,
  projectId,
  moduleId,
  peekKey,
  onPeekChange,
  peekProjectId = null,
  compact,
}: ModuleDetailScreenProps): React.ReactElement {
  const client = getAppClient();
  const entity = { scope: "module", projectId, moduleId } as const;
  const { data: projects } = useSuspenseQuery(projectsQueryOptions(client, workspaceSlug));
  const project = projects.find((entry) => entry.id === projectId) ?? null;
  const { data: module } = useSuspenseQuery(moduleDetailQueryOptions(client, workspaceSlug, projectId, moduleId));
  const { data: preferences } = useSuspenseQuery(prefsQueryOptions(client, workspaceSlug, entity));
  const showSubIssues = selectDisplayPrefs(preferences).showSubIssues;
  // The badge counts the loaded, unfiltered list total, so a guest's
  // scoped-empty read hides it while a member's matches the detail.
  const { data: firstPage } = useSuspenseQuery(
    listPageQueryOptions(client, workspaceSlug, {
      entity,
      groupBy: null,
      orderBy: "sort_order",
      showSubIssues,
    })
  );
  const { collapsed, setCollapsed } = useDetailSidebarCollapsed();
  // The header and the empty card share one add controller, so a visit
  // never mounts a second creation form or picker (MOD-031, §8.3).
  const add = useModuleAddWorkItem({
    workspaceSlug,
    projectId,
    moduleId,
    memberRole: project?.member_role ?? null,
    projectArchived: project === null || project.archived_at !== null,
  });
  useDocumentHead({
    title: project === null ? MODULES_CRUMB : moduleDetailTitle(project.name, module.name),
    robots: "noindex, nofollow",
  });

  if (project === null) {
    return (
      <section aria-label="Module details" className="flex flex-col items-center gap-(--space-2) p-(--space-12)">
        <h2 className="text-h3 text-(--text)">Project not found</h2>
        <p className="text-body text-(--text-muted)">Pick another project from the sidebar.</p>
      </section>
    );
  }

  // The API row names the membership and archive fields differently from
  // the shared dialog's option; map once, here, so the dialog can offer it.
  const createProject = toModuleCreateProject(project);
  // Edit and Delete are editor entries; guests never see them (§4.5).
  // The read-only link entries ride the same menu through `extraEntries`,
  // so every role copies and opens from the header.
  const canEdit = canEditInProject({ memberRole: project.member_role, archived: project.archived_at !== null });
  const headerMenuEntries = moduleDetailMenuEntries(workspaceSlug, projectId, moduleId);
  return (
    <div data-testid="module-detail-screen" className="flex h-full min-h-0 flex-col">
      <ModuleDetailPaletteEntries
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        module={module}
        memberRole={project.member_role}
        projectArchived={project.archived_at !== null}
      />
      <ModuleDetailCrumbs
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        projectName={project.name}
        moduleId={moduleId}
        moduleName={module.name}
      />
      <div className="flex min-h-0 flex-1 flex-col gap-(--space-4) md:flex-row">
        <div className="min-h-0 min-w-0 flex-1">
          <EntityListScreen
            workspaceSlug={workspaceSlug}
            projectId={projectId}
            entity={entity}
            title={module.name}
            moduleId={moduleId}
            completedModule={isCompletedModule(module.status)}
            peekKey={peekKey}
            onPeekChange={onPeekChange}
            peekProjectId={peekProjectId}
            compact={compact}
            extraHeaderActions={
              <>
                {add.canAdd ? (
                  <ModuleAddWorkItem
                    workspaceSlug={workspaceSlug}
                    project={createProject}
                    moduleId={moduleId}
                    controller={add}
                  />
                ) : null}
                <ModuleDetailBadge total={firstPage.total_count} />
                <ModuleSidebarToggle collapsed={collapsed} onToggle={() => setCollapsed(!collapsed)} />
                <ModuleDetailActions
                  workspaceSlug={workspaceSlug}
                  projectId={projectId}
                  moduleId={moduleId}
                  canEdit={canEdit}
                  extraEntries={headerMenuEntries}
                  archiveModule={{ id: module.id, name: module.name, status: module.status }}
                />
              </>
            }
            emptyStateActions={{
              canAct: add.canAdd,
              onCreate: add.openCreate,
              onAddExisting: add.openPicker,
            }}
          />
        </div>
        {collapsed ? null : (
          <aside className="w-full shrink-0 overflow-y-auto md:w-80">
            <ModuleAnalyticsSidebar
              workspaceSlug={workspaceSlug}
              projectId={projectId}
              moduleId={moduleId}
              progress={
                <ModuleProgressSection workspaceSlug={workspaceSlug} projectId={projectId} moduleId={moduleId} />
              }
              onClose={() => setCollapsed(true)}
            />
          </aside>
        )}
      </div>
    </div>
  );
}
