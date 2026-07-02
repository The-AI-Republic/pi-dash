/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/** Governance tier of a prompt section (mirrors the backend registry). */
export type TPromptSectionTier = "locked" | "workspace" | "overridable";

/** Prompt kinds whose recipes compose from sections. */
export type TPromptKind = "coding-task" | "review" | "scheduler";

/** Resolution scope: workspace-level, or the calling user's personal view. */
export type TPromptScope = "workspace" | "user";

/**
 * One section after override resolution. ``source`` is ``"default"``,
 * ``"workspace"``, or ``"user:<id>"``. ``editable_at_*`` express what the
 * *section* permits; the client combines them with the caller's role.
 */
export interface IResolvedSection {
  key: string;
  title: string;
  customizable: TPromptSectionTier;
  body: string;
  default_body: string;
  source: string;
  version: number;
  needs_attention: boolean;
  editable_at_workspace: boolean;
  editable_at_personal: boolean;
}

export interface IPromptSectionListResponse {
  kind: TPromptKind;
  scope: TPromptScope;
  sections: IResolvedSection[];
}

/** The assembled final template. The per-section "ingredients" come from the section-list endpoint. */
export interface IPromptCompiledResponse {
  kind: TPromptKind;
  scope: TPromptScope;
  template_body: string;
  /** Present only when the caller has personal overrides: what automatic runs get. */
  automatic_template_body?: string;
}

/** A stored override row (PUT response). */
export interface IPromptSectionOverride {
  id: string;
  workspace: string;
  user: string | null;
  section_key: string;
  body: string;
  is_active: boolean;
  version: number;
  needs_attention: boolean;
  is_workspace_level: boolean;
  updated_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface IPromptSectionUpsertPayload {
  scope: TPromptScope;
  body: string;
}

export interface IPromptPreviewPayload {
  issue_id?: string;
  binding_id?: string;
  scope?: TPromptScope;
  /** Preview an unsaved draft of this section (with `body`) instead of the saved one. */
  section_key?: string;
  body?: string;
}

export interface IPromptPreviewResponse {
  kind: TPromptKind;
  prompt: string;
}
