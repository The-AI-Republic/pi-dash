// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers, list tab (parity/specs/agents.md §2–§3, AGT-007,
// AGT-010, AGT-012). Thin composition: the schedulers feature owns the
// list, its admin gate, the install dialog and the edit/uninstall
// dialogs. A row opens the install detail route; the management cells
// open their dialog without navigating.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { usePermissions } from "../../../../../../core/session/index.js";
import { useProjectRoles, useWorkspaceMembership } from "../../../../../../features/auth/index.js";
import { isProjectAdmin } from "../../../../../../features/projects/index.js";
import {
  EditInstallDialog,
  InstallsList,
  InstallDialog,
  UninstallInstallDialog,
  useSchedulerBindings,
} from "../../../../../../features/schedulers/index.js";
import type { SchedulerBinding } from "@pidash/api-client";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers/list")({
  component: ProjectSchedulersListComponent,
});

function ProjectSchedulersListComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const navigate = useNavigate();
  const permissions = usePermissions(ws);
  const roles = useProjectRoles(ws);
  const membership = useWorkspaceMembership(ws);
  const bindings = useSchedulerBindings(ws, projectId);
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<SchedulerBinding | null>(null);
  const [uninstalling, setUninstalling] = React.useState<SchedulerBinding | null>(null);

  // Management needs both standing reads to succeed and the project role
  // to be admin; a failed read refuses rather than enabling controls.
  const canManage = roles.isSuccess && membership.isSuccess && isProjectAdmin(roles.data[projectId] ?? null);
  const installed = bindings.data ?? [];
  const installedIds = installed.map((binding) => binding.scheduler);

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
        onEditInstall={setEditing}
        onUninstallInstall={setUninstalling}
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
      <EditInstallDialog
        workspaceSlug={ws}
        projectId={projectId}
        binding={editing}
        onClose={() => setEditing(null)}
        onSaved={() => {
          void bindings.refetch();
        }}
      />
      <UninstallInstallDialog
        workspaceSlug={ws}
        projectId={projectId}
        binding={uninstalling}
        onClose={() => setUninstalling(null)}
      />
    </>
  );
}
