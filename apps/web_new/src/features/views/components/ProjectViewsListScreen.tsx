// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project views list screen (parity/specs/views.md §3, VIEW-001/002).
// Reads the project row for the feature gate, the saved-views collection
// for the list, and the roster for owner chips; sets the tab title; and
// composes the header with exactly one body state. No list fetch runs
// while the gate shows.

import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useWorkspace } from "../../../core/session/index.js";
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
  sortProjectViewsDefault,
} from "../lib/projectViews.js";
import { ProjectViewsGate } from "./ProjectViewsGate.js";
import { ProjectViewsError } from "./ProjectViewsError.js";
import { ProjectViewsListBody } from "./ProjectViewsListBody.js";
import { ProjectViewsListHeader } from "./ProjectViewsListHeader.js";
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
  const [createOpen, setCreateOpen] = React.useState(false);
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
  const sorted = sortProjectViewsDefault(fetched);
  const state = resolveProjectViewsBody({
    viewsPending: views.isPending,
    viewsError: views.isError,
    viewCount: fetched.length,
    // NEWFRONT-720 owns the filtered set; until it lands everything fetched stays visible.
    visibleCount: sorted.length,
  });
  // The header and the zero state share one opener (VIEW-003/009): the
  // explicit prop when a host overrides it, else the screen's own dialog.
  const openCreateView = onCreateView ?? (() => setCreateOpen(true));
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
      />
      <ProjectViewsListBody
        state={state}
        views={sorted}
        members={members.data}
        workspaceSlug={workspaceSlug}
        projectId={projectId}
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
