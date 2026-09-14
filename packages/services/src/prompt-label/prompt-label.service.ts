/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IPromptLabel,
  IPromptLabelAssignmentPayload,
  IPromptLabelCreatePayload,
  IPromptLabelListResponse,
  IPromptLabelUpdatePayload,
} from "@pi-dash/types";
import { APIService } from "../api.service";

/**
 * Workspace-scoped prompt **label** management (PDASHOSS01-71).
 *
 * Backed by the Django REST endpoints under ``/api/workspaces/<slug>/``:
 * ``prompt-labels`` (list + create), ``prompt-labels/<id>`` (update/delete),
 * and ``prompt-labels/<id>/assignments`` (attach/detach to a section/receipt).
 * Reads are any active member; writes are admin-gated on the server.
 */
export class PromptLabelService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  async list(workspaceSlug: string): Promise<IPromptLabelListResponse> {
    return this.get(`/api/workspaces/${workspaceSlug}/prompt-labels`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async create(workspaceSlug: string, payload: IPromptLabelCreatePayload): Promise<IPromptLabel> {
    return this.post(`/api/workspaces/${workspaceSlug}/prompt-labels`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async update(workspaceSlug: string, labelId: string, payload: IPromptLabelUpdatePayload): Promise<IPromptLabel> {
    return this.patch(`/api/workspaces/${workspaceSlug}/prompt-labels/${labelId}`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async remove(workspaceSlug: string, labelId: string): Promise<void> {
    return this.delete(`/api/workspaces/${workspaceSlug}/prompt-labels/${labelId}`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async attach(workspaceSlug: string, labelId: string, payload: IPromptLabelAssignmentPayload): Promise<IPromptLabel> {
    return this.post(`/api/workspaces/${workspaceSlug}/prompt-labels/${labelId}/assignments`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async detach(workspaceSlug: string, labelId: string, payload: IPromptLabelAssignmentPayload): Promise<void> {
    return this.delete(
      `/api/workspaces/${workspaceSlug}/prompt-labels/${labelId}/assignments` +
        `?target_type=${encodeURIComponent(payload.target_type)}&target_key=${encodeURIComponent(payload.target_key)}`
    )
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
