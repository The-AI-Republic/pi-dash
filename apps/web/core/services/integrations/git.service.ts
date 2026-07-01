/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IGitCodeReviewLink,
  IGitCodeReviewLinkCreateRequest,
  IGitProjectBindRequest,
  IGitProjectBindingStatus,
  IGitProviderAccount,
  IGitProviderAccountCreateRequest,
  IGitProviderAccountsResponse,
  IGitProvidersResponse,
  IGitReposPage,
} from "@pi-dash/types";
import { APIService } from "@/services/api.service";

export class GitIntegrationService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  async listProviders(workspaceSlug: string): Promise<IGitProvidersResponse> {
    return this.get(`/api/workspaces/${workspaceSlug}/integrations/git/providers/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async listAccounts(workspaceSlug: string): Promise<IGitProviderAccountsResponse> {
    return this.get(`/api/workspaces/${workspaceSlug}/integrations/git/accounts/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async createAccount(workspaceSlug: string, data: IGitProviderAccountCreateRequest): Promise<IGitProviderAccount> {
    return this.post(`/api/workspaces/${workspaceSlug}/integrations/git/accounts/`, data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async disconnectAccount(workspaceSlug: string, accountId: string): Promise<{ connected: boolean }> {
    return this.delete(`/api/workspaces/${workspaceSlug}/integrations/git/accounts/${accountId}/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async listRepos(workspaceSlug: string, accountId: string, page: number = 1): Promise<IGitReposPage> {
    return this.get(`/api/workspaces/${workspaceSlug}/integrations/git/accounts/${accountId}/repos/`, {
      params: { page },
    })
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async getProjectRepository(workspaceSlug: string, projectId: string): Promise<IGitProjectBindingStatus> {
    return this.get(`/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async bindProjectRepository(
    workspaceSlug: string,
    projectId: string,
    data: IGitProjectBindRequest
  ): Promise<IGitProjectBindingStatus> {
    return this.post(`/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/bind/`, data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async setProjectSyncEnabled(
    workspaceSlug: string,
    projectId: string,
    enabled: boolean
  ): Promise<IGitProjectBindingStatus> {
    return this.patch(`/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/`, { enabled })
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async unbindProjectRepository(workspaceSlug: string, projectId: string): Promise<{ bound: boolean }> {
    return this.delete(`/api/workspaces/${workspaceSlug}/projects/${projectId}/repository/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async listIssueCodeReviews(workspaceSlug: string, projectId: string, issueId: string): Promise<IGitCodeReviewLink[]> {
    return this.get(`/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/code-reviews/`)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async attachIssueCodeReview(
    workspaceSlug: string,
    projectId: string,
    issueId: string,
    data: IGitCodeReviewLinkCreateRequest
  ): Promise<IGitCodeReviewLink> {
    return this.post(`/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/code-reviews/`, data)
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async detachIssueCodeReview(
    workspaceSlug: string,
    projectId: string,
    issueId: string,
    linkId: string
  ): Promise<void> {
    return this.delete(
      `/api/workspaces/${workspaceSlug}/projects/${projectId}/issues/${issueId}/code-reviews/${linkId}/`
    )
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }
}
