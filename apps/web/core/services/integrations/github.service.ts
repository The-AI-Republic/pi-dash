/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type { IGithubConnectionStatus, IGithubConnectRequest, IGithubReposPage } from "@pi-dash/types";
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
}
