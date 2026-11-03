// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bare project-settings path (PSET-002, from
// parity/specs/project-settings-shell.md). The area has no landing page
// of its own: the view forwards to the first joined project in sidebar
// order, or, for a viewer without one, explains the area and opens the
// project-creation dialog. The forward reads the cached project
// collection only — no extra request fires.

import type { Project } from "@pidash/api-client";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Navigate } from "@tanstack/react-router";
import * as React from "react";

import { CreateProjectDialog } from "../../../../../features/projects/components/CreateProjectDialog.js";
import { projectsQueryOptions } from "../../../../../features/projects/api/queries.js";
import { selectSidebarProjects } from "../../../../../features/projects/sidebar.js";
import { workspaceMembersQueryOptions } from "../../../../../features/projects/api/members.js";
import { ProjectSettingsGuidance } from "../../../../../features/project-settings/components/ProjectSettingsGuidance.js";
import { Route as GatedLayoutRoute } from "../../route.js";

export const Route = createFileRoute("/$ws/_gated/settings/projects/")({
  component: ProjectsSettingsLanding,
});

/**
 * First joined project's settings address, or null when the viewer holds
 * no membership: only joined, unarchived rows count, in sidebar order.
 */
export function selectForwardTarget(
  projects: readonly Pick<Project, "id" | "member_role" | "archived_at" | "sort_order">[],
  workspaceSlug: string
): string | null {
  const first = selectSidebarProjects(projects as readonly Project[])[0];
  return first === undefined ? null : `/${workspaceSlug}/settings/projects/${first.id}`;
}

function ProjectsSettingsLanding(): React.ReactElement {
  const { ws } = Route.useParams();
  const { apiClient } = GatedLayoutRoute.useRouteContext();
  const projects = useQuery(projectsQueryOptions(apiClient, ws));
  const [createOpen, setCreateOpen] = React.useState(false);

  if (projects.isPending) return <div data-testid="project-settings-landing" />;

  const target = projects.isSuccess ? selectForwardTarget(projects.data, ws) : null;
  if (target !== null) return <Navigate to={target} replace />;

  return (
    <div data-testid="project-settings-landing">
      <ProjectSettingsGuidance onCreate={() => setCreateOpen(true)} />
      <ProjectCreateMount workspaceSlug={ws} open={createOpen} onOpenChange={setCreateOpen} />
    </div>
  );
}

function ProjectCreateMount({
  workspaceSlug,
  open,
  onOpenChange,
}: {
  workspaceSlug: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement {
  const { apiClient } = GatedLayoutRoute.useRouteContext();
  const members = useQuery(workspaceMembersQueryOptions(apiClient, workspaceSlug));
  return (
    <CreateProjectDialog workspaceSlug={workspaceSlug} members={members.data} open={open} onOpenChange={onOpenChange} />
  );
}
