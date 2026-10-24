// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings schedulers (parity/specs/project-settings-part-f.md,
// PSET-055–061): the installs table behind the project-admin gate, inside
// the settings shell, plus the two-path install dialog and the prefilled
// edit / uninstall pair the rows open. Members read the installs list on
// the project page; this address refuses them while the shell keeps its
// rail and page chrome.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import { isAdminStanding } from "../../../../../../features/project-settings/tabs.js";
import { ProjectSettingsPage } from "../../../../../../features/project-settings/components/ProjectSettingsPage.js";
import {
  ProjectSettingsShell,
  useProjectSettings,
} from "../../../../../../features/project-settings/components/ProjectSettingsLayout.js";
import { EditInstallDialog } from "../../../../../../features/schedulers/EditInstallDialog.js";
import { InstallsList } from "../../../../../../features/schedulers/InstallsList.js";
import { InstallDialog } from "../../../../../../features/schedulers/InstallDialog.js";
import { UninstallInstallDialog } from "../../../../../../features/schedulers/UninstallInstallDialog.js";
import { useSchedulerBindings } from "../../../../../../features/schedulers/installData.js";
import type { SchedulerBinding } from "@pidash/api-client";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/schedulers")({
  component: ProjectSchedulersSettingsComponent,
});

function ProjectSchedulersSettingsComponent(): React.ReactElement {
  const { ws, projectId } = Route.useParams();
  return (
    <ProjectSettingsShell workspaceSlug={ws} projectId={projectId}>
      <SchedulersSettingsBody workspaceSlug={ws} projectId={projectId} />
    </ProjectSettingsShell>
  );
}

function SchedulersSettingsBody({
  workspaceSlug,
  projectId,
}: {
  workspaceSlug: string;
  projectId: string;
}): React.ReactElement {
  const { role, workspaceAdmin } = useProjectSettings();
  if (!isAdminStanding(role)) {
    return (
      <section aria-label="Schedulers" className="flex flex-col gap-(--space-3) px-(--space-6) py-(--space-8)">
        <h1 className="text-h2 text-(--text)">Oops! You are not authorized to view this page</h1>
        <p className="text-body text-(--text-muted)">Only project admins can manage these settings.</p>
      </section>
    );
  }
  return (
    <ProjectSettingsPage title="Schedulers">
      <SchedulersSettingsPanel workspaceSlug={workspaceSlug} projectId={projectId} canAuthor={workspaceAdmin} />
    </ProjectSettingsPage>
  );
}

function SchedulersSettingsPanel({
  workspaceSlug,
  projectId,
  canAuthor,
}: {
  workspaceSlug: string;
  projectId: string;
  canAuthor: boolean;
}): React.ReactElement {
  const navigate = useNavigate();
  const bindings = useSchedulerBindings(workspaceSlug, projectId);
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<SchedulerBinding | null>(null);
  const [uninstalling, setUninstalling] = React.useState<SchedulerBinding | null>(null);

  return (
    <>
      <InstallsList
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        variant="settings"
        onOpenInstall={(binding: SchedulerBinding) => {
          void navigate({ href: `/${workspaceSlug}/projects/${projectId}/schedulers/${binding.id}` });
        }}
        onCreate={() => setCreating(true)}
        onEditInstall={setEditing}
        onUninstallInstall={setUninstalling}
      />
      <InstallDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        open={creating}
        onClose={() => setCreating(false)}
        canInstall
        canAuthor={canAuthor}
        installedSchedulerIds={(bindings.data ?? []).map((binding: SchedulerBinding) => binding.scheduler)}
        onInstalled={() => {
          void bindings.refetch();
        }}
      />
      <EditInstallDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={editing}
        onClose={() => setEditing(null)}
        onSaved={() => {
          void bindings.refetch();
        }}
      />
      <UninstallInstallDialog
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        binding={uninstalling}
        onClose={() => setUninstalling(null)}
      />
    </>
  );
}
