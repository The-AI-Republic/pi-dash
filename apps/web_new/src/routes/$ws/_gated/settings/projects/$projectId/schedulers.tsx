// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project-settings schedulers (parity/specs/agents.md §2, AGT-007): the
// project page's install panel behind the project-admin gate, inside the
// settings shell. Members read the installs list on the project page; this
// address refuses them, while the shell keeps its rail and page chrome.

import { createFileRoute, useNavigate } from "@tanstack/react-router";
import * as React from "react";

import {
  isAdminStanding,
  ProjectSettingsPage,
  ProjectSettingsShell,
  useProjectSettings,
} from "../../../../../../features/project-settings/index.js";
import { InstallsList, InstallDialog, useSchedulerBindings } from "../../../../../../features/schedulers/index.js";
import type { SchedulerBinding } from "@pidash/api-client";

export const Route = createFileRoute("/$ws/_gated/settings/projects/$projectId/schedulers")({
  component: ProjectSchedulersSettingsComponent,
});

export function ProjectSchedulersSettingsComponent(): React.ReactElement {
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
    </>
  );
}
