// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace integrations page (parity/specs/workspace-settings.md §15–§17,
// WSET-016–018). Admin-only: the settings layout already refuses members
// and guests, and this page repeats the check so it stays correct when
// mounted on its own. The shell owns the section frame; this page renders
// the informational banner naming the section above one card per
// supported git provider, and each card owns its own read, its own
// writes and its own notices.

import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePermissions } from "../../../core/session/index.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { SettingsNotAuthorized } from "../components/SettingsNotAuthorized.js";
import {
  GITHUB_CONNECTED_NOTICE,
  GITHUB_CONNECTED_NOTICE_BODY,
  GITHUB_CONNECT_FAILED_BODY,
  GITHUB_CONNECT_FAILED_NOTICE,
  GITHUB_DISCONNECTED_NOTICE,
  GITHUB_DISCONNECTED_NOTICE_BODY,
  GITHUB_DISCONNECT_FAILED_BODY,
  GITHUB_DISCONNECT_FAILED_NOTICE,
  GITHUB_LOADING_STATUS,
  GithubCard,
} from "./GithubCard.js";
import {
  GITLAB_CONNECTED_NOTICE,
  GITLAB_CONNECTED_NOTICE_BODY,
  GITLAB_CONNECT_FAILED_BODY,
  GITLAB_CONNECT_FAILED_NOTICE,
  GITLAB_DISCONNECTED_NOTICE,
  GITLAB_DISCONNECTED_NOTICE_BODY,
  GITLAB_DISCONNECT_FAILED_BODY,
  GITLAB_DISCONNECT_FAILED_NOTICE,
  GitlabCard,
} from "./GitlabCard.js";
import { ProviderCard } from "./ProviderCard.js";
import {
  gitProviderAccountsQueryOptions,
  GITHUB_PROVIDER_NAME,
  githubIntegrationQueryOptions,
  useConnectGithub,
  useDisconnectGithub,
  useDisconnectGitlab,
  useRegisterGitlab,
} from "./integrationQueries.js";

export const INTEGRATIONS_HEADING = "Integrations";
export const INTEGRATIONS_BANNER =
  "Connect the code host your projects use. Credentials are verified before we store them, and project repositories mirror issues from there.";

export interface IntegrationsPageProps {
  workspaceSlug: string;
}

export function IntegrationsPage({ workspaceSlug }: IntegrationsPageProps): React.ReactElement {
  const permissions = usePermissions(workspaceSlug);
  // The layout refuses members and guests before this page mounts; the
  // repeat check keeps a direct mount honest. A loading role renders the
  // frame's quiet state rather than flashing provider content or a denial.
  if (permissions.isLoading) return <IntegrationsPending />;
  if (!permissions.isAdmin) return <SettingsNotAuthorized workspaceSlug={workspaceSlug} />;
  return <IntegrationsScreen workspaceSlug={workspaceSlug} />;
}

function IntegrationsPending(): React.ReactElement {
  return (
    <div
      data-testid="workspace-settings-integrations-pending"
      aria-busy="true"
      className="flex flex-col gap-(--space-6)"
    >
      <IntegrationsBanner />
      <ProviderCard provider="github" name={GITHUB_PROVIDER_NAME} status={GITHUB_LOADING_STATUS} />
    </div>
  );
}

/** The informational banner: the section heading and what connecting does. */
function IntegrationsBanner(): React.ReactElement {
  return (
    <header className="flex flex-col gap-(--space-1)">
      <h2 className="text-h3 text-(--text)">{INTEGRATIONS_HEADING}</h2>
      <p className="text-body max-w-xl text-(--text-muted)">{INTEGRATIONS_BANNER}</p>
    </header>
  );
}

export function IntegrationsScreen({ workspaceSlug }: IntegrationsPageProps): React.ReactElement {
  const client = getAppClient();
  const github = useQuery(githubIntegrationQueryOptions(client, workspaceSlug));
  const accounts = useQuery(gitProviderAccountsQueryOptions(client, workspaceSlug));
  const connectGithub = useConnectGithub(workspaceSlug);
  const disconnectGithub = useDisconnectGithub(workspaceSlug);
  const registerGitlab = useRegisterGitlab(workspaceSlug);
  const disconnectGitlab = useDisconnectGitlab(workspaceSlug);

  const connectGithubToken = React.useCallback(
    (token: string): Promise<boolean> =>
      connectGithub.mutateAsync(token).then(
        () => {
          notifySuccess(GITHUB_CONNECTED_NOTICE, GITHUB_CONNECTED_NOTICE_BODY);
          return true;
        },
        () => {
          notifyError(GITHUB_CONNECT_FAILED_NOTICE, GITHUB_CONNECT_FAILED_BODY);
          return false;
        }
      ),
    [connectGithub]
  );

  const disconnectGithubConnection = React.useCallback((): void => {
    disconnectGithub.mutate(undefined, {
      onSuccess: () => {
        notifySuccess(GITHUB_DISCONNECTED_NOTICE, GITHUB_DISCONNECTED_NOTICE_BODY);
      },
      onError: () => {
        notifyError(GITHUB_DISCONNECT_FAILED_NOTICE, GITHUB_DISCONNECT_FAILED_BODY);
      },
    });
  }, [disconnectGithub]);

  const connectGitlabAccount = React.useCallback(
    (input: { token: string; hostUrl: string }): Promise<boolean> =>
      registerGitlab.mutateAsync(input).then(
        () => {
          notifySuccess(GITLAB_CONNECTED_NOTICE, GITLAB_CONNECTED_NOTICE_BODY);
          return true;
        },
        () => {
          notifyError(GITLAB_CONNECT_FAILED_NOTICE, GITLAB_CONNECT_FAILED_BODY);
          return false;
        }
      ),
    [registerGitlab]
  );

  const disconnectGitlabAccount = React.useCallback(
    (accountId: string): void => {
      disconnectGitlab.mutate(accountId, {
        onSuccess: () => {
          notifySuccess(GITLAB_DISCONNECTED_NOTICE, GITLAB_DISCONNECTED_NOTICE_BODY);
        },
        onError: () => {
          notifyError(GITLAB_DISCONNECT_FAILED_NOTICE, GITLAB_DISCONNECT_FAILED_BODY);
        },
      });
    },
    [disconnectGitlab]
  );

  return (
    <div data-testid="workspace-settings-integrations" className="flex max-w-3xl flex-col gap-(--space-6)">
      <IntegrationsBanner />
      <div className="flex flex-col gap-(--space-4)">
        <GithubCard
          status={github.data}
          statusPending={github.isPending}
          connectPending={connectGithub.isPending}
          disconnectPending={disconnectGithub.isPending}
          onConnect={connectGithubToken}
          onDisconnect={disconnectGithubConnection}
          readFailed={github.isError}
          onRetry={() => {
            void github.refetch();
          }}
        />
        <GitlabCard
          accounts={accounts.data?.accounts}
          accountsPending={accounts.isPending}
          connectPending={registerGitlab.isPending}
          disconnectPending={disconnectGitlab.isPending}
          onConnect={connectGitlabAccount}
          onDisconnect={disconnectGitlabAccount}
          readFailed={accounts.isError}
          onRetry={() => {
            void accounts.refetch();
          }}
        />
      </div>
    </div>
  );
}
