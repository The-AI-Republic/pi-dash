/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * GitHub-compatibility service (PDASHOSS01-359, design Phase 10).
 *
 * The server stores data in generic ``git_*`` tables; these GitHub-only
 * routes are kept as byte-identical compatibility surfaces. PAT/App methods
 * remain live for GitHub-specific flows (profile PAT/App cards); the generic
 * ``GitIntegrationService`` owns accounts/repos for new code. Issue PR methods
 * are kept for compatibility — new callers should use
 * ``GitIntegrationService`` code-review methods. Responses do not change.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IGithubAppInstallationStatus,
  IGithubAppInstallStartRequest,
  IGithubAppInstallStartResponse,
  IGithubAppRefreshRequest,
  IGithubAppStatus,
  IGithubConnectionStatus,
  IGithubConnectRequest,
  IGithubPullRequestLink,
  IGithubPullRequestLinkCreateRequest,
  IGithubReposPage,
} from "@pi-dash/types";
import { APIService } from "@/services/api.service";

export class GithubIntegrationService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  async getStatus(workspaceSlug: string): Promise<IGithubConnectionStatus> {
    return this.get(`/api/workspaces/${workspaceSlug}/integrations/github/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async connectWorkspace(workspaceSlug: string, data: IGithubConnectRequest): Promise<IGithubConnectionStatus> {
    return this.post(`/api/workspaces/${workspaceSlug}/integrations/github/connect/`, data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async disconnectWorkspace(workspaceSlug: string): Promise<IGithubConnectionStatus> {
    return this.post(`/api/workspaces/${workspaceSlug}/integrations/github/disconnect/`, {})
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async listRepos(workspaceSlug: string, page: number = 1): Promise<IGithubReposPage> {
    return this.get(`/api/workspaces/${workspaceSlug}/integrations/github/repos/`, { params: { page } })
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async getAppStatus(): Promise<IGithubAppStatus> {
    return this.get("/api/users/me/integrations/github/app/")
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async startAppInstall(data: IGithubAppInstallStartRequest): Promise<IGithubAppInstallStartResponse> {
    return this.post("/api/users/me/integrations/github/app/install/", data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async refreshAppConnection(data: IGithubAppRefreshRequest): Promise<IGithubAppInstallationStatus> {
    return this.post("/api/users/me/integrations/github/app/refresh/", data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async listIssuePullRequests(
    workspaceSlug: string,
    projectId: string,
    issueId: string
  ): Promise<IGithubPullRequestLink[]> {
    return this.get(`/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/github-pull-requests/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async attachIssuePullRequest(
    workspaceSlug: string,
    projectId: string,
    issueId: string,
    data: IGithubPullRequestLinkCreateRequest
  ): Promise<IGithubPullRequestLink> {
    return this.post(
      `/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/github-pull-requests/`,
      data
    )
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async detachIssuePullRequest(
    workspaceSlug: string,
    projectId: string,
    issueId: string,
    linkId: string
  ): Promise<void> {
    return this.delete(
      `/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/github-pull-requests/${linkId}/`
    )
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }
}
