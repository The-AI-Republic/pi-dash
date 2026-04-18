/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash imports
import { API_BASE_URL } from "@apple-pi-dash/constants";
import { EIssueServiceType } from "@apple-pi-dash/types";
import type { TDescriptionVersionsListResponse, TDescriptionVersionDetails, TIssueServiceType } from "@apple-pi-dash/types";
// helpers
// services
import { APIService } from "@/services/api.service";

export class WorkItemVersionService extends APIService {
  private serviceType: TIssueServiceType;

  constructor(serviceType: TIssueServiceType = EIssueServiceType.WORK_ITEMS) {
    super(API_BASE_URL);
    this.serviceType = serviceType;
  }

  async listDescriptionVersions(
    workspaceSlug: string,
    projectId: string,
    workItemId: string
  ): Promise<TDescriptionVersionsListResponse> {
    return this.get(
      `/api/workspaces/${workspaceSlug}/projects/${projectId}/${this.serviceType}/${workItemId}/description-versions/`
    )
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }

  async retrieveDescriptionVersion(
    workspaceSlug: string,
    projectId: string,
    workItemId: string,
    versionId: string
  ): Promise<TDescriptionVersionDetails> {
    return this.get(
      `/api/workspaces/${workspaceSlug}/projects/${projectId}/${this.serviceType}/${workItemId}/description-versions/${versionId}/`
    )
      .then((response) => response?.data)
      .catch((error) => {
        throw error?.response?.data;
      });
  }
}
