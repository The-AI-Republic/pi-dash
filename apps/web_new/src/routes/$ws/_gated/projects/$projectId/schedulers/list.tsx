// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers, list tab (parity/specs/agents.md §2, AGT-007).
// Thin composition: the schedulers feature owns the list, its admin gate
// and the install dialog. A row opens the install detail route that
// NEWFRONT-590 lands; the address is stable from here on.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { usePermissions } from "../../../../../../core/session/index.js";
import { useProjectRoles, useWorkspaceMembership } from "../../../../../../features/auth/index.js";
import { isProjectAdmin } from "../../../../../../features/projects/index.js";
import { InstallsList, InstallDialog, useSchedulerBindings } from "../../../../../../features/schedulers/index.js";
import type { SchedulerBinding } from "@pidash/api-client";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers/list")({
  component: ProjectSchedulersListComponent,
});

export function ProjectSchedulersListComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const navigate = useNavigate();
  const permissions = usePermissions(ws);
  const roles = useProjectRoles(ws);
  const membership = useWorkspaceMembership(ws);
  const bindings = useSchedulerBindings(ws, projectId);
  const [creating, setCreating] = React.useState(false);

  // Management needs both standing reads to succeed and the project role
  // to be admin; a failed read refuses rather than enabling controls.
  const canManage = roles.isSuccess && membership.isSuccess && isProjectAdmin(roles.data?.[projectId] ?? null);
  const installedIds = (bindings.data ?? []).map((binding) => binding.scheduler);

  function openInstall(binding: SchedulerBinding): void {
    void navigate({ href: `/${ws}/projects/${projectId}/schedulers/${binding.id}` });
  }

  return (
    <>
      <InstallsList
        workspaceSlug={ws}
        projectId={projectId}
        onOpenInstall={openInstall}
        onCreate={() => setCreating(true)}
      />
      <InstallDialog
        workspaceSlug={ws}
        projectId={projectId}
        open={creating}
        onClose={() => setCreating(false)}
        canInstall={canManage}
        canAuthor={permissions.isAdmin}
        installedSchedulerIds={installedIds}
        onInstalled={() => {
          void bindings.refetch();
        }}
      />
    </>
  );
}
