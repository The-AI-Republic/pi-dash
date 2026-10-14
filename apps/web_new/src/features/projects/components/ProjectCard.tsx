// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project card shell (SHELL-024). The grid cell: a link into the
// project's work-items view labelled by the project name. NEWFRONT-283
// extends this file in place with identity, visibility, members,
// favorites and quick actions; the link target and name heading stay.
import { Link } from "@tanstack/react-router";
import * as React from "react";
import type { ProjectDetails } from "@pidash/api-client";

export interface ProjectCardProps {
  workspaceSlug: string;
  project: Pick<ProjectDetails, "id" | "name">;
}

export function ProjectCard({ workspaceSlug, project }: ProjectCardProps): React.ReactElement {
  return (
    <Link
      to="/$ws/projects/$projectId/issues"
      params={{ ws: workspaceSlug, projectId: project.id }}
      search={{}}
      data-testid="project-card"
      className="block rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4) hover:border-(--accent)"
    >
      <h2 className="text-emphasis truncate text-(--text)">{project.name}</h2>
    </Link>
  );
}
