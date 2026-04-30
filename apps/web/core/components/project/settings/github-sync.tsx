/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { useParams } from "next/navigation";
import useSWR, { mutate } from "swr";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Loader, ToggleSwitch } from "@pi-dash/ui";
// constants
import { GITHUB_INTEGRATION_STATUS, GITHUB_PROJECT_BINDING } from "@/constants/fetch-keys";
// hooks
import { useUserPermissions } from "@/hooks/store/user";
// services
import { GithubIntegrationService } from "@/services/integrations/github.service";
import { ProjectService } from "@/services/project";

const githubService = new GithubIntegrationService();
const projectService = new ProjectService();

export function ProjectGithubSyncSection() {
  const params = useParams();
  const workspaceSlug = params.workspaceSlug?.toString();
  const projectId = params.projectId?.toString();

  const { allowPermissions } = useUserPermissions();
  const isProjectAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.PROJECT);

  const [submitting, setSubmitting] = useState(false);

  const wsKey = workspaceSlug ? GITHUB_INTEGRATION_STATUS(workspaceSlug) : null;
  const bindingKey = projectId ? GITHUB_PROJECT_BINDING(projectId) : null;

  const { data: wsStatus } = useSWR(wsKey, () => (workspaceSlug ? githubService.getStatus(workspaceSlug) : null));

  const { data: binding } = useSWR(bindingKey, () =>
    workspaceSlug && projectId ? projectService.getGithubBindingStatus(workspaceSlug, projectId) : null
  );

  const handleToggle = async (enabled: boolean) => {
    if (!workspaceSlug || !projectId) return;
    setSubmitting(true);
    try {
      await projectService.setGithubSyncEnabled(workspaceSlug, projectId, enabled);
      await mutate(bindingKey);
    } catch (e: any) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Toggle failed",
        message: e?.error || "Could not change sync state.",
      });
    } finally {
      setSubmitting(false);
    }
  };

  const handleUnbind = async () => {
    if (!workspaceSlug || !projectId) return;
    setSubmitting(true);
    try {
      await projectService.unbindGithubRepository(workspaceSlug, projectId);
      await mutate(bindingKey);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "Unbound",
        message: "Repository unlinked. Mirrored issues remain editable.",
      });
    } catch (e: any) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Unbind failed",
        message: e?.error || "Could not unbind repository.",
      });
    } finally {
      setSubmitting(false);
    }
  };

  if (wsStatus !== undefined && !wsStatus.connected) {
    return (
      <div className="flex flex-col gap-2 rounded-md border border-subtle bg-surface-1 p-4">
        <h3 className="text-body-sm-medium">GitHub</h3>
        <p className="text-body-xs-regular text-secondary">
          Connect a GitHub credential at the workspace level (Workspace settings → Integrations) to bind repositories to
          projects.
        </p>
      </div>
    );
  }

  if (wsStatus === undefined || binding === undefined) {
    return (
      <Loader className="flex flex-col gap-2 p-4">
        <Loader.Item height="20px" width="160px" />
        <Loader.Item height="32px" width="100%" />
      </Loader>
    );
  }

  if (binding.bound && binding.repository) {
    return (
      <div className="flex flex-col gap-3 rounded-md border border-subtle bg-surface-1 p-4">
        <div className="flex items-start justify-between gap-2">
          <div>
            <h3 className="text-body-sm-medium">GitHub</h3>
            <p className="text-body-xs-regular text-secondary">
              Bound to{" "}
              <a className="text-primary underline" href={binding.repository.url} target="_blank" rel="noreferrer">
                {binding.repository.owner}/{binding.repository.name}
              </a>
              . Mirrored issues are read-only on title, description, and synced comment bodies. Workflow fields stay
              editable.
            </p>
          </div>
          <Button variant="error-outline" size="sm" disabled={!isProjectAdmin || submitting} onClick={handleUnbind}>
            Unbind
          </Button>
        </div>

        <div className="flex items-center gap-3">
          <ToggleSwitch
            value={!!binding.is_sync_enabled}
            onChange={() => handleToggle(!binding.is_sync_enabled)}
            disabled={!isProjectAdmin || submitting}
          />
          <span className="text-body-xs-regular">
            Sync issues every 4 hours
            {binding.last_synced_at ? ` — last sync ${new Date(binding.last_synced_at).toLocaleString()}` : ""}
          </span>
        </div>

        {binding.last_sync_error ? (
          <p className="text-danger text-body-xs-regular">Last error: {binding.last_sync_error}</p>
        ) : null}
      </div>
    );
  }

  // Not bound — Bind itself happens in General Settings (next to the
  // `repo_url` field) so the project's stored URL and its actual binding
  // can never drift apart.
  return (
    <div className="flex flex-col gap-2 rounded-md border border-subtle bg-surface-1 p-4">
      <h3 className="text-body-sm-medium">GitHub</h3>
      <p className="text-body-xs-regular text-secondary">
        Set the project's <strong>Git repository URL</strong> in General Settings and click <strong>Bind</strong>. Once
        a repo is bound here, you can toggle sync on/off without leaving this page.
      </p>
    </div>
  );
}
