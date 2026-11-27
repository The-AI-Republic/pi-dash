// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project schedulers, list tab (parity/specs/agents.md §2–§3, AGT-007,
// AGT-012). Thin composition: the schedulers feature owns the list, its
// admin gate and the edit/uninstall dialogs. Creation is not here: the
// section's tab row carries it for every tab (route.tsx, AGT-010). A row
// opens the install detail route; the management cells open their dialog
// without navigating.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { EditInstallDialog } from "../../../../../../features/schedulers/EditInstallDialog.js";
import { InstallsList } from "../../../../../../features/schedulers/InstallsList.js";
import { UninstallInstallDialog } from "../../../../../../features/schedulers/UninstallInstallDialog.js";
import { useSchedulerBindings } from "../../../../../../features/schedulers/installData.js";
import type { SchedulerBinding } from "@pidash/api-client";

export const Route = createFileRoute("/$ws/_gated/projects/$projectId/schedulers/list")({
  component: ProjectSchedulersListComponent,
});

function ProjectSchedulersListComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  const navigate = useNavigate();
  const bindings = useSchedulerBindings(ws, projectId);
  const [editing, setEditing] = React.useState<SchedulerBinding | null>(null);
  const [uninstalling, setUninstalling] = React.useState<SchedulerBinding | null>(null);

  function openInstall(binding: SchedulerBinding): void {
    void navigate({ href: `/${ws}/projects/${projectId}/schedulers/${binding.id}` });
  }

  return (
    <>
      <InstallsList
        workspaceSlug={ws}
        projectId={projectId}
        onOpenInstall={openInstall}
        onEditInstall={setEditing}
        onUninstallInstall={setUninstalling}
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
