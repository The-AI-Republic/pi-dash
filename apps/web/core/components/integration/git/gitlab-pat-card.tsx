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
// assets
import GitlabLogo from "@/app/assets/logos/gitlab-logo.svg?url";
// constants
import { GIT_PROVIDER_ACCOUNTS } from "@/constants/fetch-keys";
// hooks
import { useUserPermissions } from "@/hooks/store/user";
// services
import { GitIntegrationService } from "@/services/integrations/git.service";

const gitService = new GitIntegrationService();

export function GitlabPatCard() {
  const { workspaceSlug } = useParams();
  const { allowPermissions } = useUserPermissions();
  const isUserAdmin = allowPermissions([EUserPermissions.ADMIN], EUserPermissionsLevel.WORKSPACE);

  const [token, setToken] = useState("");
  const [hostUrl, setHostUrl] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const swrKey = workspaceSlug ? GIT_PROVIDER_ACCOUNTS(workspaceSlug.toString()) : null;
  const { data } = useSWR(swrKey, () => (workspaceSlug ? gitService.listAccounts(workspaceSlug.toString()) : null));
  const account = data?.accounts.find((item) => item.provider === "gitlab" && item.status !== "revoked");

  const handleConnect = async () => {
    if (!workspaceSlug || !token.trim()) return;
    setSubmitting(true);
    try {
      await gitService.createAccount(workspaceSlug.toString(), {
        provider: "gitlab",
        token: token.trim(),
        host_url: hostUrl.trim() || undefined,
        auth_type: "pat",
      });
      setToken("");
      await mutate(swrKey);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "Connected",
        message: "GitLab credential verified.",
      });
    } catch (e: any) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Connection failed",
        message: e?.error || "GitLab rejected the token.",
      });
    } finally {
      setSubmitting(false);
    }
  };

  const handleDisconnect = async () => {
    if (!workspaceSlug || !account) return;
    setSubmitting(true);
    try {
      await gitService.disconnectAccount(workspaceSlug.toString(), account.id);
      await mutate(swrKey);
      setToast({
        type: TOAST_TYPE.SUCCESS,
        title: "Disconnected",
        message: "GitLab account disconnected; project syncs using it are disabled.",
      });
    } catch (e: any) {
      setToast({
        type: TOAST_TYPE.ERROR,
        title: "Disconnect failed",
        message: e?.error || "Could not disconnect GitLab.",
      });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="flex flex-col gap-3 border-b border-subtle bg-surface-1 px-4 py-6">
      <div className="flex items-start gap-4">
        <div className="h-10 w-10 flex-shrink-0">
          <img src={GitlabLogo} className="h-full w-full object-contain" alt="GitLab Logo" />
        </div>
        <div className="flex-1">
          <h3 className="flex items-center gap-2 text-body-xs-medium">
            GitLab
            {account && <CheckCircle className="h-3.5 w-3.5 fill-transparent text-success-primary" />}
          </h3>
          <p className="text-body-xs-regular text-secondary">
            {!data
              ? "Loading..."
              : account
                ? `Connected as ${account.external_account_login || account.display_name || "GitLab"} on ${account.host_url}.`
                : "Paste a GitLab token with API access. Pi Dash can bind GitLab projects and mirror issues and notes."}
          </p>
        </div>
      </div>

      {!data ? (
        <Loader>
          <Loader.Item height="32px" width="180px" />
        </Loader>
      ) : account ? (
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
        <div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-end">
          <Input
            type="url"
            placeholder="https://gitlab.com"
            value={hostUrl}
            onChange={(e) => setHostUrl(e.target.value)}
            disabled={!isUserAdmin || submitting}
            className="lg:w-64"
          />
          <Input
            type="password"
            placeholder="glpat-..."
            value={token}
            onChange={(e) => setToken(e.target.value)}
            disabled={!isUserAdmin || submitting}
            className="lg:w-72"
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
