// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Developer-integration reads and writes (parity/specs/workspace-settings.md
// §15–§17, WSET-016–018). GitHub keeps one workspace-wide connection;
// GitLab is represented by provider-account rows, and the card keeps the
// first non-revoked GitLab row as its connected account. Reads follow the
// settings fetch policy; a successful write re-reads its own card instead
// of trusting the answered shape.

import { queryOptions, useMutation, useQueryClient } from "@tanstack/react-query";

import {
  connectGithubIntegration,
  disconnectGithubIntegration,
  disconnectGitProviderAccount,
  getGithubIntegration,
  listGitProviderAccounts,
  registerGitProviderAccount,
  type ApiClient,
  type GitProviderAccount,
  type GithubIntegrationStatus,
} from "@pidash/api-client";

import { getAppClient, getProviderWriteClient } from "../../../core/api/client.js";
import { workspaceScope } from "../../../core/query/index.js";
import { WORKSPACE_SETTINGS_QUERY_POLICY } from "../lib/settingsQuery.js";

export const GITHUB_PROVIDER_NAME = "GitHub";
export const GITLAB_PROVIDER_NAME = "GitLab";
export const GITLAB_PROVIDER_KEY = "gitlab";

export const integrationsKeys = {
  all: (workspaceSlug: string) => [...workspaceScope(workspaceSlug), "workspace-settings", "integrations"] as const,
  github: (workspaceSlug: string) => [...integrationsKeys.all(workspaceSlug), "github"] as const,
  accounts: (workspaceSlug: string) => [...integrationsKeys.all(workspaceSlug), "accounts"] as const,
};

export function githubIntegrationQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions<GithubIntegrationStatus>({
    queryKey: integrationsKeys.github(workspaceSlug),
    queryFn: () => getGithubIntegration(client, workspaceSlug),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
  });
}

export function gitProviderAccountsQueryOptions(client: ApiClient, workspaceSlug: string) {
  return queryOptions({
    queryKey: integrationsKeys.accounts(workspaceSlug),
    queryFn: () => listGitProviderAccounts(client, workspaceSlug),
    ...WORKSPACE_SETTINGS_QUERY_POLICY,
  });
}

/** The account the GitLab card represents: the first row that is not revoked. */
export function selectActiveGitlabAccount(accounts: readonly GitProviderAccount[]): GitProviderAccount | null {
  return accounts.find((account) => account.provider === GITLAB_PROVIDER_KEY && account.status !== "revoked") ?? null;
}

/** Display identity of a connected account, falling back display name → provider name. */
export function accountIdentity(account: GitProviderAccount): string {
  const login = account.external_account_login.trim();
  if (login !== "") return login;
  const displayName = account.display_name.trim();
  if (displayName !== "") return displayName;
  return account.provider === GITLAB_PROVIDER_KEY ? GITLAB_PROVIDER_NAME : GITHUB_PROVIDER_NAME;
}

/** GitHub's verified login, or the provider name when the payload omits it. */
export function githubIdentity(status: GithubIntegrationStatus): string {
  const login = status.github_user_login?.trim() ?? "";
  return login === "" ? GITHUB_PROVIDER_NAME : login;
}

/** Whether the Connect control may run: a non-blank token and no pending write. */
export function canSubmitToken(token: string, pending: boolean): boolean {
  return !pending && token.trim().length > 0;
}

/** Whether disconnecting is meaningful for a connected GitHub workspace. */
export function canDisconnectGithub(status: GithubIntegrationStatus | undefined, pending: boolean): boolean {
  return !pending && status?.connected === true;
}

async function refreshGithub(queryClient: ReturnType<typeof useQueryClient>, workspaceSlug: string): Promise<void> {
  await queryClient.refetchQueries({ queryKey: integrationsKeys.github(workspaceSlug) });
}

async function refreshAccounts(queryClient: ReturnType<typeof useQueryClient>, workspaceSlug: string): Promise<void> {
  await queryClient.refetchQueries({ queryKey: integrationsKeys.accounts(workspaceSlug) });
}

export function useConnectGithub(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (token: string) =>
      connectGithubIntegration(getProviderWriteClient(), workspaceSlug, { token: token.trim() }),
    onSuccess: () => refreshGithub(queryClient, workspaceSlug),
  });
}

export function useDisconnectGithub(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => disconnectGithubIntegration(getAppClient(), workspaceSlug),
    onSuccess: () => refreshGithub(queryClient, workspaceSlug),
  });
}

export interface RegisterGitlabInput {
  token: string;
  /** Trimmed host; omitted when the field was left blank. */
  hostUrl: string;
}

export function useRegisterGitlab(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ token, hostUrl }: RegisterGitlabInput) =>
      registerGitProviderAccount(getProviderWriteClient(), workspaceSlug, {
        provider: "gitlab",
        token: token.trim(),
        auth_type: "pat",
        ...(hostUrl.trim() === "" ? {} : { host_url: hostUrl.trim() }),
      }),
    onSuccess: () => refreshAccounts(queryClient, workspaceSlug),
  });
}

export function useDisconnectGitlab(workspaceSlug: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (accountId: string) => disconnectGitProviderAccount(getAppClient(), workspaceSlug, accountId),
    onSuccess: () => refreshAccounts(queryClient, workspaceSlug),
  });
}
