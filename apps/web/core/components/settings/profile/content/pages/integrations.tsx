/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import { observer } from "mobx-react";
import useSWR from "swr";
import { CheckCircle, ExternalLink, RefreshCw } from "lucide-react";
// pi dash imports
import { Button } from "@pi-dash/propel/button";
import { setToast, TOAST_TYPE } from "@pi-dash/propel/toast";
import type { IGithubAppWorkspaceStatus } from "@pi-dash/types";
import { Loader } from "@pi-dash/ui";
// components
import { ProfileSettingsHeading } from "@/components/settings/profile/heading";
// constants
import { GITHUB_APP_STATUS } from "@/constants/fetch-keys";
// services
import { GithubIntegrationService } from "@/services/integrations/github.service";

const githubService = new GithubIntegrationService();

const formatDateTime = (value?: string | null) => {
  if (!value) return "Never";
  return new Date(value).toLocaleString();
};

const installErrorMessage: Record<string, string> = {
  actor_mismatch: "This install session was started by a different Pi Dash user.",
  github_verification_failed: "Pi Dash could not verify the GitHub installation.",
  installation_not_visible_to_user: "Your GitHub account cannot access that installation.",
  missing_installation_id: "GitHub did not return an installation id.",
  missing_oauth_code: "GitHub did not return a setup authorization code.",
  workspace_admin_required: "You must be a workspace admin to connect GitHub.",
};

function InstallCallbackToast({ onWorkspaceSlug }: { onWorkspaceSlug: (workspaceSlug: string) => void }) {
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const result = params.get("github_app");
    if (!result) return;

    if (result === "connected") {
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "GitHub connected",
        message: "The GitHub App installation was verified.",
      });
      const workspaceSlug = params.get("workspace_slug");
      if (workspaceSlug) onWorkspaceSlug(workspaceSlug);
    } else {
      const error = params.get("error") || result;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "GitHub connection failed",
        message: installErrorMessage[error] || "The GitHub App installation could not be completed.",
      });
    }

    params.delete("github_app");
    params.delete("workspace_slug");
    params.delete("error");
    const nextQuery = params.toString();
    const nextUrl = `${window.location.pathname}${nextQuery ? `?${nextQuery}` : ""}${window.location.hash}`;
    window.history.replaceState(null, "", nextUrl);
  }, [onWorkspaceSlug]);

  return null;
}

function GithubAppWorkspacePanel({
  configured,
  selectedWorkspace,
  onInstall,
  onRefresh,
  installing,
  refreshing,
}: {
  configured: boolean;
  selectedWorkspace: IGithubAppWorkspaceStatus;
  onInstall: () => void;
  onRefresh: () => void;
  installing: boolean;
  refreshing: boolean;
}) {
  const installation = selectedWorkspace.github_app;
  const isConnected = installation.connected;
  const isSuspended = !!installation.suspended_at;

  return (
    <div className="flex flex-col gap-4 rounded-md border border-subtle bg-surface-1 px-4 py-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="truncate text-14 font-medium text-primary">GitHub App</h3>
            {isConnected && !isSuspended ? (
              <CheckCircle className="h-4 w-4 shrink-0 fill-transparent text-success-primary" />
            ) : null}
          </div>
          <p className="mt-1 text-12 text-secondary">
            {isConnected
              ? `Installed for ${installation.account_login || "GitHub"} on ${selectedWorkspace.name}.`
              : `Install the Pi Dash GitHub App for ${selectedWorkspace.name}.`}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {isConnected ? (
            <Button
              variant="secondary"
              size="sm"
              prependIcon={<RefreshCw />}
              loading={refreshing}
              disabled={refreshing}
              onClick={onRefresh}
            >
              Refresh
            </Button>
          ) : null}
          <Button
            variant="primary"
            size="sm"
            appendIcon={<ExternalLink />}
            loading={installing}
            disabled={!configured || installing}
            onClick={onInstall}
          >
            {isConnected ? "Reconnect" : "Install"}
          </Button>
        </div>
      </div>

      {!configured ? (
        <div className="rounded-md border border-warning-subtle bg-warning-subtle px-3 py-2 text-12 text-secondary">
          GitHub App configuration is missing on this Pi Dash instance.
        </div>
      ) : null}

      {isConnected ? (
        <div className="grid gap-3 text-12 sm:grid-cols-2">
          <StatusField label="Account" value={installation.account_login || "Unknown"} />
          <StatusField label="Account type" value={installation.account_type || "Unknown"} />
          <StatusField
            label="Repository access"
            value={installation.repository_selection === "all" ? "All repositories" : "Selected repositories"}
          />
          <StatusField label="Repositories visible" value={String(installation.repository_count ?? 0)} />
          <StatusField label="Verified" value={formatDateTime(installation.verified_at)} />
          <StatusField label="Last checked" value={formatDateTime(installation.last_checked_at)} />
        </div>
      ) : (
        <p className="text-12 text-secondary">
          No GitHub App installation is connected for this workspace yet. Pi Dash will verify installation access after
          GitHub redirects you back.
        </p>
      )}

      {isSuspended ? (
        <div className="rounded-md border border-danger-subtle bg-danger-subtle px-3 py-2 text-12 text-danger-primary">
          This GitHub App installation is suspended or removed in GitHub.
        </div>
      ) : null}

      {installation.last_check_error ? (
        <div className="rounded-md border border-danger-subtle bg-danger-subtle px-3 py-2 text-12 text-danger-primary">
          Last check failed: {installation.last_check_error}
        </div>
      ) : null}
    </div>
  );
}

function StatusField({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-md bg-surface-2 px-3 py-2">
      <div className="text-11 text-tertiary">{label}</div>
      <div className="truncate text-12 font-medium text-primary">{value}</div>
    </div>
  );
}

export const IntegrationsProfileSettings = observer(function IntegrationsProfileSettings() {
  const { data, mutate, isLoading } = useSWR(GITHUB_APP_STATUS, () => githubService.getAppStatus());
  const [selectedWorkspaceSlug, setSelectedWorkspaceSlug] = useState("");
  const [installing, setInstalling] = useState(false);
  const [refreshing, setRefreshing] = useState(false);

  const selectedWorkspace = useMemo(
    () => data?.workspaces.find((workspace) => workspace.slug === selectedWorkspaceSlug) ?? data?.workspaces[0],
    [data?.workspaces, selectedWorkspaceSlug]
  );

  useEffect(() => {
    if (!selectedWorkspaceSlug && data?.workspaces[0]) {
      setSelectedWorkspaceSlug(data.workspaces[0].slug);
    }
  }, [data?.workspaces, selectedWorkspaceSlug]);

  const startInstall = async () => {
    if (!selectedWorkspace) return;
    setInstalling(true);
    try {
      const response = await githubService.startAppInstall({ workspace_slug: selectedWorkspace.slug });
      window.location.assign(response.install_url);
    } catch (e: unknown) {
      const error = e as { error?: string; detail?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Install failed",
        message: error?.error || error?.detail || "Could not start the GitHub App install flow.",
      });
      setInstalling(false);
    }
  };

  const refreshConnection = async () => {
    if (!selectedWorkspace) return;
    setRefreshing(true);
    try {
      await githubService.refreshAppConnection({ workspace_slug: selectedWorkspace.slug });
      await mutate();
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "GitHub refreshed",
        message: "The GitHub App installation was checked successfully.",
      });
    } catch (e: unknown) {
      const error = e as { error?: string; detail?: string } | null;
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Refresh failed",
        message: error?.error || error?.detail || "Could not check the GitHub App installation.",
      });
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div className="size-full">
      <InstallCallbackToast
        onWorkspaceSlug={(workspaceSlug) => {
          setSelectedWorkspaceSlug(workspaceSlug);
          void mutate();
        }}
      />
      <ProfileSettingsHeading
        title="Integrations"
        description="Connect external services to workspaces you administer."
      />

      <div className="mt-7 flex max-w-3xl flex-col gap-5">
        {isLoading || !data ? (
          <Loader className="flex flex-col gap-3">
            <Loader.Item height="76px" width="100%" />
            <Loader.Item height="180px" width="100%" />
          </Loader>
        ) : data.workspaces.length === 0 ? (
          <div className="rounded-md border border-subtle bg-surface-1 px-4 py-5">
            <h3 className="text-14 font-medium text-primary">No admin workspaces</h3>
            <p className="mt-1 text-12 text-secondary">
              You need workspace admin access before you can install GitHub for a Pi Dash workspace.
            </p>
          </div>
        ) : (
          <>
            <label className="flex max-w-sm flex-col gap-1 text-13">
              <span className="text-secondary">Workspace</span>
              <select
                value={selectedWorkspace?.slug ?? ""}
                onChange={(event) => setSelectedWorkspaceSlug(event.target.value)}
                className="rounded-md border border-subtle bg-surface-1 px-3 py-2 text-primary"
              >
                {data.workspaces.map((workspace) => (
                  <option key={workspace.id} value={workspace.slug}>
                    {workspace.name}
                  </option>
                ))}
              </select>
            </label>

            {selectedWorkspace ? (
              <GithubAppWorkspacePanel
                configured={data.configured}
                selectedWorkspace={selectedWorkspace}
                onInstall={startInstall}
                onRefresh={refreshConnection}
                installing={installing}
                refreshing={refreshing}
              />
            ) : null}
          </>
        )}
      </div>
    </div>
  );
});
