/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import type { IUserLite } from "./users";

// All the app integrations that are available
export interface IAppIntegration {
  author: string;
  avatar_url: string | null;
  created_at: string;
  created_by: string | null;
  description: any;
  id: string;
  metadata: any;
  network: number;
  provider: string;
  redirect_url: string;
  title: string;
  updated_at: string;
  updated_by: string | null;
  verified: boolean;
  webhook_secret: string;
  webhook_url: string;
}

export interface IWorkspaceIntegration {
  actor: string;
  api_token: string;
  config: any;
  created_at: string;
  created_by: string;
  id: string;
  integration: string;
  integration_detail: IAppIntegration;
  metadata: any;
  updated_at: string;
  updated_by: string;
  workspace: string;
}

// GitHub Issue Sync (.ai_design/github_sync/design.md)

export interface IGithubConnectionStatus {
  connected: boolean;
  github_user_login?: string;
  verified_at?: string;
}

export interface IGithubConnectRequest {
  token: string;
}

export interface IGithubRepoSummary {
  id: number;
  owner: string;
  name: string;
  full_name: string;
  default_branch: string;
  private: boolean;
}

export interface IGithubReposPage {
  repos: IGithubRepoSummary[];
  page: number;
  has_next_page: boolean;
}

export interface IGithubProjectBindRequest {
  repo_url: string;
}

export interface IGithubProjectBindingStatus {
  bound: boolean;
  id?: string;
  repository?: {
    id: number;
    owner: string;
    name: string;
    url: string;
  };
  is_sync_enabled?: boolean;
  last_synced_at?: string | null;
  last_sync_error?: string;
  repo_url?: string;
}

// GitHub App Enablement (.ai_design/github_deep_integration/design.md)

export interface IGithubAppInstallationStatus {
  connected: boolean;
  installation_id?: number;
  account_login?: string;
  account_type?: "User" | "Organization" | "Unknown";
  repository_selection?: "all" | "selected";
  repository_count?: number;
  permissions?: Record<string, string>;
  events?: string[];
  installed_at?: string | null;
  suspended_at?: string | null;
  verified_at?: string | null;
  last_checked_at?: string | null;
  last_check_error?: string;
}

export interface IGithubAppWorkspaceStatus {
  id: string;
  slug: string;
  name: string;
  github_app: IGithubAppInstallationStatus;
}

export interface IGithubAppStatus {
  configured: boolean;
  app_slug: string;
  workspaces: IGithubAppWorkspaceStatus[];
}

export interface IGithubAppInstallStartRequest {
  workspace_slug: string;
}

export interface IGithubAppInstallStartResponse {
  state: string;
  expires_at: string;
  install_url: string;
}

export type IGithubAppRefreshRequest = IGithubAppInstallStartRequest;

// GitHub Pull Request links (.ai_design/github_pr_issue_link/design.md)

export type TGithubPullRequestState = "open" | "closed";

export interface IGithubPullRequestLink {
  id: string;
  issue: string;
  repo_owner: string;
  repo_name: string;
  pr_number: number;
  url: string;
  title: string;
  state: TGithubPullRequestState;
  merged: boolean;
  draft: boolean;
  pr_updated_at: string | null;
  created_at: string;
  updated_at: string;
  created_by: string | null;
  created_by_detail?: IUserLite | null;
}

export interface IGithubPullRequestLinkCreateRequest {
  url: string;
}

// slack integration
export interface ISlackIntegration {
  id: string;
  created_at: string;
  updated_at: string;
  access_token: string;
  scopes: string;
  bot_user_id: string;
  webhook_url: string;
  data: ISlackIntegrationData;
  team_id: string;
  team_name: string;
  created_by: string;
  updated_by: string;
  project: string;
  workspace: string;
  workspace_integration: string;
}

export interface ISlackIntegrationData {
  ok: boolean;
  team: {
    id: string;
    name: string;
  };
  scope: string;
  app_id: string;
  enterprise: any;
  token_type: string;
  authed_user: string;
  bot_user_id: string;
  access_token: string;
  incoming_webhook: {
    url: string;
    channel: string;
    channel_id: string;
    configuration_url: string;
  };
  is_enterprise_install: boolean;
}
