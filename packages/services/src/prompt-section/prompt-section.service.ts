/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import type {
  IPromptCompiledResponse,
  IPromptPreviewPayload,
  IPromptPreviewResponse,
  IPromptSectionListResponse,
  IPromptSectionOverride,
  IPromptSectionUpsertPayload,
  TPromptKind,
  TPromptScope,
} from "@pi-dash/types";
import { APIService } from "../api.service";

/**
 * Workspace-scoped prompt **section** customization (design §7.2).
 *
 * Backed by the Django REST endpoints under ``/api/workspaces/<slug>/``:
 * ``prompt-sections`` (list + per-section upsert/revert) and
 * ``prompts/<kind>/compiled|preview``. Reads are any active member; workspace-
 * scope writes are admin-gated and previews are admin-gated on the server.
 */
export class PromptSectionService extends APIService {
  constructor(BASE_URL?: string) {
    super(BASE_URL || API_BASE_URL);
  }

  async list(workspaceSlug: string, kind: TPromptKind, scope: TPromptScope): Promise<IPromptSectionListResponse> {
    return this.get(`/api/workspaces/${workspaceSlug}/prompt-sections`, { params: { kind, scope } })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async upsert(
    workspaceSlug: string,
    sectionKey: string,
    payload: IPromptSectionUpsertPayload
  ): Promise<IPromptSectionOverride> {
    return this.put(`/api/workspaces/${workspaceSlug}/prompt-sections/${sectionKey}`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async revert(workspaceSlug: string, sectionKey: string, scope: TPromptScope): Promise<void> {
    return this.delete(`/api/workspaces/${workspaceSlug}/prompt-sections/${sectionKey}?scope=${scope}`)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async compiled(workspaceSlug: string, kind: TPromptKind, scope: TPromptScope): Promise<IPromptCompiledResponse> {
    return this.get(`/api/workspaces/${workspaceSlug}/prompts/${kind}/compiled`, { params: { scope } })
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }

  async preview(
    workspaceSlug: string,
    kind: TPromptKind,
    payload: IPromptPreviewPayload
  ): Promise<IPromptPreviewResponse> {
    return this.post(`/api/workspaces/${workspaceSlug}/prompts/${kind}/preview`, payload)
      .then((res) => res?.data)
      .catch((err) => {
        throw err?.response?.data;
      });
  }
}
