/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export interface IPromptTemplate {
  id: string;
  workspace: string | null;
  name: string;
  body: string;
  is_active: boolean;
  version: number;
  is_global_default: boolean;
  updated_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface IPromptTemplateCreatePayload {
  name?: string;
  body?: string;
}

export interface IPromptTemplateUpdatePayload {
  body: string;
}

export interface IPromptTemplatePreviewPayload {
  issue_id: string;
  body?: string;
}

export interface IPromptTemplatePreviewResponse {
  prompt: string;
}
