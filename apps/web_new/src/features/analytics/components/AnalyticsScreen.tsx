// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Analytics screen (parity/specs/analytics.md §§1, 3–7, 8,
// ANL-001–008). Workspace shell: section breadcrumb,
// workspace-naming title, pending state, no-project empty view,
// and the toolbar (tab strip plus picker) over the selected tab's
// panel. The overview panel pairs its headline grid with the radar
// and active-projects sections; unknown segments keep the strip
// and picker with no panel.

import { useQuery } from "@tanstack/react-query";
import { Breadcrumbs, Button, Skeleton } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePermissions, useWorkspace } from "../../../core/session/hooks.js";
import { projectsQueryOptions } from "../../projects/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { resolveAnalyticsScope, selectAnalyticsProjects } from "../lib/scope.js";
import { selectAnalyticsTab } from "../lib/tabs.js";
import { ActiveProjectsSection } from "./ActiveProjectsSection.js";
import { AnalyticsEmptyView } from "./AnalyticsEmptyView.js";
import { AnalyticsPicker } from "./AnalyticsPicker.js";
import { AnalyticsTabStrip } from "./AnalyticsTabStrip.js";
import { HeadlineCards } from "./HeadlineCards.js";
import { RadarSection } from "./RadarSection.js";

export interface AnalyticsScreenProps {
  workspaceSlug: string;
  /** Raw path segment; unknown values select no panel. */
  tabId: string;
  /** Address scope in selection order. */
  projectIds: readonly string[];
  onScopeChange: (ids: string[]) => void;
}

export function AnalyticsScreen({
  workspaceSlug,
  tabId,
  projectIds,
  onScopeChange,
}: AnalyticsScreenProps): React.ReactElement {
  const projects = useQuery(projectsQueryOptions(getAppClient(), workspaceSlug));
  const { workspace } = useWorkspace(workspaceSlug);
  const { canEdit } = usePermissions(workspaceSlug);
  useDocumentHead({ title: `${workspace?.name ?? workspaceSlug} - Analytics`, robots: "noindex, nofollow" });

  const tab = selectAnalyticsTab(tabId);
  const selectable = projects.data === undefined ? [] : selectAnalyticsProjects(projects.data);
  const scope = resolveAnalyticsScope(projectIds, selectable);

  return (
    <div data-testid="analytics-screen" className="flex min-h-0 flex-1 flex-col gap-(--space-4) p-(--space-6)">
      <header>
        <Breadcrumbs entries={[{ label: "Analytics" }]} />
      </header>
      {projects.isPending ? (
        <div data-testid="analytics-pending" className="flex flex-col gap-(--space-3)">
          <Skeleton height="40px" />
          <Skeleton height="120px" />
        </div>
      ) : projects.isError ? (
        <div data-testid="analytics-error" className="flex flex-col items-start gap-(--space-3)">
          <p className="text-body text-(--text)">Couldn&apos;t load projects.</p>
          <Button
            variant="secondary"
            onClick={() => {
              void projects.refetch();
            }}
          >
            Try again
          </Button>
        </div>
      ) : selectable.length === 0 ? (
        <AnalyticsEmptyView workspaceSlug={workspaceSlug} canCreate={canEdit} />
      ) : (
        <>
          <div className="flex items-start gap-(--space-3)">
            <AnalyticsTabStrip workspaceSlug={workspaceSlug} tabId={tabId} scopeIds={projectIds} />
            <div className="shrink-0">
              <AnalyticsPicker
                workspaceSlug={workspaceSlug}
                projects={selectable.map((project) => ({ id: project.id, name: project.name }))}
                value={projectIds}
                onChange={onScopeChange}
              />
            </div>
          </div>
          {tab === null ? null : (
            <section
              role="tabpanel"
              id={`analytics-panel-${tab}`}
              aria-labelledby={`analytics-tab-${tab}`}
              data-testid="analytics-panel"
              className="flex min-w-0 flex-1 flex-col gap-(--space-6)"
            >
              <HeadlineCards workspaceSlug={workspaceSlug} tab={tab} projectIds={scope} />
              {tab === "overview" ? (
                <div className="grid gap-(--space-6) xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
                  <RadarSection workspaceSlug={workspaceSlug} projectIds={scope} projects={selectable} />
                  <ActiveProjectsSection workspaceSlug={workspaceSlug} projectIds={scope} projects={selectable} />
                </div>
              ) : null}
            </section>
          )}
        </>
      )}
    </div>
  );
}
