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
import { Input, Loader, ToggleSwitch } from "@pi-dash/ui";
import type { IGithubRepoSummary } from "@pi-dash/types";
// constants
import {
  GITHUB_INTEGRATION_REPOS,
  GITHUB_INTEGRATION_STATUS,
  GITHUB_PROJECT_BINDING,
} from "@/constants/fetch-keys";
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

  const [filter, setFilter] = useState("");
  const [page, setPage] = useState(1);
  const [submitting, setSubmitting] = useState(false);

  const wsKey = workspaceSlug ? GITHUB_INTEGRATION_STATUS(workspaceSlug) : null;
  const bindingKey = projectId ? GITHUB_PROJECT_BINDING(projectId) : null;
  const reposKey = workspaceSlug ? GITHUB_INTEGRATION_REPOS(workspaceSlug, page) : null;

  const { data: wsStatus } = useSWR(wsKey, () => (workspaceSlug ? githubService.getStatus(workspaceSlug) : null));

  const { data: binding } = useSWR(bindingKey, () =>
    workspaceSlug && projectId ? projectService.getGithubBindingStatus(workspaceSlug, projectId) : null
  );

  const reposEnabled = !!wsStatus?.connected && !binding?.bound;
  const { data: repos } = useSWR(reposEnabled ? reposKey : null, () =>
    workspaceSlug ? githubService.listRepos(workspaceSlug, page) : null
  );

  const handleBind = async (repo: IGithubRepoSummary) => {
    if (!workspaceSlug || !projectId) return;
    setSubmitting(true);
    try {
      await projectService.bindGithubRepository(workspaceSlug, projectId, {
        repository_id: repo.id,
        owner: repo.owner,
        name: repo.name,
        url: `https://github.com/${repo.full_name}`,
      });
      await mutate(bindingKey);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "Repository bound",
        message: `${repo.full_name} is now linked. Toggle sync on to start mirroring.`,
      });
    } catch (e: any) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Bind failed",
        message: e?.error || "Could not bind repository.",
      });
    } finally {
      setSubmitting(false);
    }
  };

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

  // Workspace integration not connected — gate the rest of the UI.
  if (wsStatus !== undefined && !wsStatus.connected) {
    return (
      <div className="flex flex-col gap-2 rounded-md border border-subtle bg-surface-1 p-4">
        <h3 className="text-body-sm-medium">GitHub</h3>
        <p className="text-body-xs-regular text-secondary">
          Connect a GitHub credential at the workspace level to bind repositories to projects.
        </p>
      </div>
    );
  }

  // Loading.
  if (wsStatus === undefined || binding === undefined) {
    return (
      <Loader className="flex flex-col gap-2 p-4">
        <Loader.Item height="20px" width="160px" />
        <Loader.Item height="32px" width="100%" />
      </Loader>
    );
  }

  // Bound: show status + toggle + unbind.
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
          <Button
            variant="error-outline"
            size="sm"
            disabled={!isProjectAdmin || submitting}
            onClick={handleUnbind}
          >
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
          <p className="text-body-xs-regular text-danger">Last error: {binding.last_sync_error}</p>
        ) : null}
      </div>
    );
  }

  // Not bound: repo picker.
  const filtered = (repos?.repos || []).filter((r) =>
    r.full_name.toLowerCase().includes(filter.trim().toLowerCase())
  );

  return (
    <div className="flex flex-col gap-3 rounded-md border border-subtle bg-surface-1 p-4">
      <div>
        <h3 className="text-body-sm-medium">GitHub</h3>
        <p className="text-body-xs-regular text-secondary">
          Pick a repository to mirror open issues into this project. One repository per project; unbind to switch.
        </p>
      </div>

      <Input
        placeholder="Filter repositories..."
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        disabled={!isProjectAdmin}
      />

      {repos === undefined ? (
        <Loader>
          <Loader.Item height="32px" width="100%" />
        </Loader>
      ) : (
        <ul className="flex max-h-72 flex-col divide-y divide-subtle overflow-y-auto rounded-md border border-subtle">
          {filtered.length === 0 ? (
            <li className="px-3 py-2 text-body-xs-regular text-secondary">No repositories match.</li>
          ) : (
            filtered.map((repo) => (
              <li key={repo.id} className="flex items-center justify-between gap-2 px-3 py-2">
                <span className="text-body-xs-regular">
                  {repo.full_name}
                  {repo.private ? <span className="ml-2 text-tertiary">(private)</span> : null}
                </span>
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={!isProjectAdmin || submitting}
                  onClick={() => handleBind(repo)}
                >
                  Bind
                </Button>
              </li>
            ))
          )}
        </ul>
      )}

      <div className="flex items-center justify-between">
        <Button
          size="sm"
          variant="tertiary"
          disabled={page <= 1 || submitting}
          onClick={() => setPage((p) => Math.max(1, p - 1))}
        >
          Previous
        </Button>
        <span className="text-body-xs-regular text-secondary">Page {page}</span>
        <Button
          size="sm"
          variant="tertiary"
          disabled={!repos?.has_next_page || submitting}
          onClick={() => setPage((p) => p + 1)}
        >
          Next
        </Button>
      </div>
    </div>
  );
}
