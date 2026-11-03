/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/** What a prompt label can be attached to: a section (by `section_key`) or a receipt (by prompt `kind`). */
export type TPromptLabelTargetType = "section" | "receipt";

/** One attachment of a label to a section/receipt target. */
export interface IPromptLabelTarget {
  id: string;
  target_type: TPromptLabelTargetType;
  target_key: string;
  created_at: string;
}

/**
 * A workspace-scoped, user-managed prompt label plus its section/receipt
 * attachments. `targets` lets the client build both the per-target chip map
 * (target → labels) and the click-to-filter map (label → targets).
 */
export interface IPromptLabel {
  id: string;
  workspace: string;
  name: string;
  color: string;
  targets: IPromptLabelTarget[];
  created_by: string | null;
  updated_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface IPromptLabelListResponse {
  labels: IPromptLabel[];
}

export interface IPromptLabelCreatePayload {
  name: string;
  color?: string;
}

export interface IPromptLabelUpdatePayload {
  name?: string;
  color?: string;
}

export interface IPromptLabelAssignmentPayload {
  target_type: TPromptLabelTargetType;
  target_key: string;
}
