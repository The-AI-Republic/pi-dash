/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IPromptTemplate,
  IPromptTemplateCreatePayload,
  IPromptTemplatePreviewPayload,
  IPromptTemplatePreviewResponse,
  IPromptTemplateUpdatePayload,
} from "@pi-dash/types";
import { APIService } from "../api.service";

/**
 * Workspace-scoped prompt template CRUD + preview.
 *
 * Backed by the Django REST endpoints under
 * ``/api/workspaces/<slug>/prompt-templates``. Read access (``list``,
 * ``retrieve``) is any active workspace member; ``create``, ``update``,
 * ``archive``, and ``preview`` are workspace-admin-gated on the server.
 */
export class PromptTemplateService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  async list(workspaceSlug: string): Promise<IPromptTemplate[]> {
    return this.get(`/api/workspaces/${workspaceSlug}/prompt-templates`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async retrieve(workspaceSlug: string, templateId: string): Promise<IPromptTemplate> {
    return this.get(`/api/workspaces/${workspaceSlug}/prompt-templates/${templateId}`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async create(workspaceSlug: string, payload: IPromptTemplateCreatePayload = {}): Promise<IPromptTemplate> {
    return this.post(`/api/workspaces/${workspaceSlug}/prompt-templates`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async update(
    workspaceSlug: string,
    templateId: string,
    payload: IPromptTemplateUpdatePayload
  ): Promise<IPromptTemplate> {
    return this.patch(`/api/workspaces/${workspaceSlug}/prompt-templates/${templateId}`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async archive(workspaceSlug: string, templateId: string): Promise<IPromptTemplate> {
    return this.post(`/api/workspaces/${workspaceSlug}/prompt-templates/${templateId}/archive`, {})
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async preview(
    workspaceSlug: string,
    templateId: string,
    payload: IPromptTemplatePreviewPayload
  ): Promise<IPromptTemplatePreviewResponse> {
    return this.post(`/api/workspaces/${workspaceSlug}/prompt-templates/${templateId}/preview`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
