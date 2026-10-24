// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module detail screen (parity/specs/modules.md §8, MOD-029, MOD-030,
// MOD-032). The modules-owned frame around the shared entity host:
// the breadcrumb trail with the module switcher, the work-item count
// badge and sidebar toggle in the desktop header, the tab title, and
// the analytics sidebar beside the hosted layouts. The gone-away gate
// wraps this screen at the route; guests render the same frame with
// the host and sidebar refusing their edits.

import { useSuspenseQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { isCompletedModule } from "../../../shared/pickers/modules.js";
import { EntityListScreen, listPageQueryOptions, prefsQueryOptions, selectDisplayPrefs } from "../../issues/index.js";
import { moduleDetailTitle } from "../lib/moduleDetail.js";
import { MODULES_CRUMB } from "../lib/moduleList.js";
import { useDetailSidebarCollapsed } from "../lib/detailSidebarStore.js";
import { ModuleAnalyticsSidebar } from "./sidebar/ModuleAnalyticsSidebar.js";
import { ModuleProgressSection } from "./sidebar/ModuleProgressSection.js";
import { moduleDetailQueryOptions } from "./sidebar/moduleSidebarQueries.js";
import { ModuleDetailBadge } from "./ModuleDetailBadge.js";
import { ModuleDetailCrumbs } from "./ModuleDetailCrumbs.js";
import { ModuleSidebarToggle } from "./ModuleSidebarToggle.js";

export interface ModuleDetailScreenProps {
  workspaceSlug: string;
  projectId: string;
  moduleId: string;
  peekKey: string | null;
  onPeekChange: (key: string | null) => void;
  compact: boolean;
}

export function ModuleDetailScreen({
  workspaceSlug,
  projectId,
  moduleId,
  peekKey,
  onPeekChange,
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

  return (
    <div data-testid="module-detail-screen" className="flex h-full min-h-0 flex-col">
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
            compact={compact}
            extraHeaderActions={
              <>
                <ModuleDetailBadge total={firstPage.total_count} />
                <ModuleSidebarToggle collapsed={collapsed} onToggle={() => setCollapsed(!collapsed)} />
              </>
            }
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
