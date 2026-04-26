/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useState } from "react";
import { useParams } from "next/navigation";
import useSWR, { mutate } from "swr";
import { CheckCircle } from "lucide-react";
import { EUserPermissions, EUserPermissionsLevel } from "@pi-dash/constants";
import { Button } from "@pi-dash/propel/button";
import { TOAST_TYPE, setToast } from "@pi-dash/propel/toast";
import { Input, Loader } from "@pi-dash/ui";
import type { IAppIntegration } from "@pi-dash/types";
// assets
import GithubLogo from "@/app/assets/services/github.png?url";
// constants
import { GITHUB_INTEGRATION_STATUS } from "@/constants/fetch-keys";
// hooks
import { useUserPermissions } from "@/hooks/store/user";
// services
import { GithubIntegrationService } from "@/services/integrations/github.service";

type Props = {
  integration: IAppIntegration;
};

const githubService = new GithubIntegrationService();

const PAT_HELP_URL =
  "https://github.com/settings/tokens?type=beta&description=Pi+Dash+Issue+Sync&scopes=public_repo";

export function GithubPatCard({ integration }: Props) {
  const { workspaceSlug } = useParams();
  const { allowPermissions } = useUserPermissions();
  const isUserAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE);

  const [token, setToken] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const swrKey = workspaceSlug ? GITHUB_INTEGRATION_STATUS(workspaceSlug.toString()) : null;

  const { data: status } = useSWR(swrKey, () =>
    workspaceSlug ? githubService.getStatus(workspaceSlug.toString()) : null
  );

  const handleConnect = async () => {
    if (!workspaceSlug || !token.trim()) return;
    setSubmitting(true);
    try {
      await githubService.connectWorkspace(workspaceSlug.toString(), { token: token.trim() });
      setToken("");
      await mutate(swrKey);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "Connected",
        message: "GitHub credential verified.",
      });
    } catch (e: any) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Connection failed",
        message: e?.error || "GitHub rejected the token.",
      });
    } finally {
      setSubmitting(false);
    }
  };

  const handleDisconnect = async () => {
    if (!workspaceSlug) return;
    setSubmitting(true);
    try {
      await githubService.disconnectWorkspace(workspaceSlug.toString());
      await mutate(swrKey);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "Disconnected",
        message: "GitHub integration removed; project syncs disabled.",
      });
    } catch (e: any) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Disconnect failed",
        message: e?.error || "Could not disconnect GitHub.",
      });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 border-b border-subtle bg-surface-1 px-4 py-6">
      <div className="flex items-start gap-4">
        <div className="h-10 w-10 flex-shrink-0">
          <img src={GithubLogo} className="h-full w-full object-cover" alt={`${integration.title} Logo`} />
        </div>
        <div className="flex-1">
          <h3 className="flex items-center gap-2 text-body-xs-medium">
            {integration.title}
            {status?.connected && <CheckCircle className="h-3.5 w-3.5 fill-transparent text-success-primary" />}
          </h3>
          <p className="text-body-xs-regular text-secondary">
            {status === undefined
              ? "Loading..."
              : status.connected
                ? `Connected as ${status.github_user_login}. Bind a repository on a project's Settings → GitHub.`
                : "Paste a GitHub fine-grained PAT with read access to issues. Pi Dash mirrors issues and comments into projects every 4 hours."}
          </p>
          <a
            className="text-body-xs-regular text-primary underline"
            href={PAT_HELP_URL}
            target="_blank"
            rel="noreferrer"
          >
            Create a fine-grained PAT
          </a>
        </div>
      </div>

      {status === undefined ? (
        <Loader>
          <Loader.Item height="32px" width="180px" />
        </Loader>
      ) : status.connected ? (
        <div className="flex items-center justify-end">
          <Button
            variant="error-fill"
            disabled={!isUserAdmin || submitting}
            loading={submitting}
            onClick={handleDisconnect}
          >
            {submitting ? "Disconnecting..." : "Disconnect"}
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-end">
          <Input
            type="password"
            placeholder="github_pat_..."
            value={token}
            onChange={(e) => setToken(e.target.value)}
            disabled={!isUserAdmin || submitting}
            className="sm:w-72"
          />
          <Button
            variant="primary"
            disabled={!isUserAdmin || submitting || !token.trim()}
            loading={submitting}
            onClick={handleConnect}
          >
            {submitting ? "Connecting..." : "Connect"}
          </Button>
        </div>
      )}
    </div>
  );
}
