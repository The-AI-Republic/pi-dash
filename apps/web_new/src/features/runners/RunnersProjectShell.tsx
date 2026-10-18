// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-scope runners shell (NEWFRONT-540, RUN-001/002/004/005). The
// project twin of the workspace shell: the same area shell, scoped to the
// project, inside the project chrome (breadcrumb header) rather than the
// issues-area access gate, because the runner area has its own denial view
// for project guests. The breadcrumb keeps the current area name (RUN-005).

import { Link, useLocation } from "@tanstack/react-router";
import { Bot } from "lucide-react";
import * as React from "react";

import { usePermissions } from "../../core/session/index.js";
import { useProjectRoles, useProjectRow } from "../auth/index.js";
import { decideProjectRunnersAccess } from "./access.js";
import { buildRunnersAreaValue, RunnersAreaProvider } from "./area.js";
import { useRunnersRail } from "./api/queries.js";
import { RunnersAreaShell } from "./components/RunnersAreaShell.js";
import { runnersBase } from "./scope.js";

/** Breadcrumb label for the project-scoped runners area (RUN-005 pins it). */
export const PROJECT_RUNNERS_CRUMB = "AI Workers";

export interface RunnersProjectShellProps {
  workspaceSlug: string;
  workspaceId: string;
  projectId: string;
  children: React.ReactNode;
}

export function RunnersProjectShell({
  workspaceSlug,
  workspaceId,
  projectId,
  children,
}: RunnersProjectShellProps): React.ReactElement | null {
  const permissions = usePermissions(workspaceSlug);
  const projectRoles = useProjectRoles(workspaceSlug);
  const project = useProjectRow(workspaceSlug, projectId);
  const access = decideProjectRunnersAccess({
    workspaceRole: permissions.role,
    workspaceLoading: permissions.isLoading,
    projectRoles: projectRoles.data,
    projectRolesLoading: projectRoles.isPending,
    projectId,
  });
  const { pathname } = useLocation();
  const rail = useRunnersRail(workspaceSlug, workspaceId, projectId, access === "allowed");
  const crumbHref = `${runnersBase({ workspaceSlug, projectId })}/`;
  // The project name is chrome, not a gate input: it renders when known and
  // leaves the denial view intact when the row is still loading.
  const chrome = (
    <header className="flex h-12 shrink-0 items-center border-b border-(--border) px-(--space-4)">
      <nav aria-label="Breadcrumb" className="text-body flex min-w-0 items-center gap-(--space-1)">
        <span className="truncate text-(--text-muted)">{project.data?.name ?? ""}</span>
        <span aria-hidden="true" className="text-(--text-muted)">
          /
        </span>
        <Link
          to={crumbHref}
          aria-current="page"
          className="flex items-center gap-(--space-2) truncate font-medium text-(--text)"
        >
          <Bot size={16} strokeWidth={1.5} aria-hidden="true" className="text-(--text-muted)" />
          {PROJECT_RUNNERS_CRUMB}
        </Link>
      </nav>
    </header>
  );
  const area = buildRunnersAreaValue({ workspaceSlug, projectId }, workspaceId, access);
  return (
    <RunnersAreaProvider value={area}>
      <RunnersAreaShell
        scope={area.scope}
        access={access}
        runners={rail.data ?? []}
        pathname={pathname}
        chrome={chrome}
      >
        {children}
      </RunnersAreaShell>
    </RunnersAreaProvider>
  );
}
