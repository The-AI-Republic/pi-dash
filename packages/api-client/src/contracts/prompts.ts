// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prompt contracts: the per-kind resolved-section list, workspace and
// personal override upserts plus reverts, the assembled (compiled)
// template receipt with its automatic-runs variant, and render previews
// against real issues or installs. Note these paths carry no trailing
// slash, unlike the rest of the workspace API.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";

export const PROMPT_SCOPES = ["user", "workspace"] as const;

export const PROMPT_TIERS = ["locked", "workspace", "overridable"] as const;

/**
 * One section after override resolution for a kind: the effective `body`
 * plus the pristine registry `default_body` for the editor diff, the
 * rung it resolved from (`default` / `workspace` / `user:<id>`), and
 * the tier capability flags the UI combines with the caller's role.
 */
export const ResolvedPromptSection = z.object({
  key: z.string(),
  title: z.string(),
  customizable: z.enum(PROMPT_TIERS),
  body: z.string(),
  default_body: z.string(),
  source: z.string(),
  version: z.number(),
  needs_attention: z.boolean(),
  editable_at_workspace: z.boolean(),
  editable_at_personal: z.boolean(),
});
export type ResolvedPromptSection = z.infer<typeof ResolvedPromptSection>;

/** The ordered section list for a kind at one scope. */
export const PromptSectionListResponse = z.object({
  kind: z.string(),
  scope: z.enum(PROMPT_SCOPES),
  sections: z.array(ResolvedPromptSection),
});
export type PromptSectionListResponse = z.infer<typeof PromptSectionListResponse>;

/**
 * GET /api/workspaces/<slug>/prompt-sections?kind=&scope=. Kind
 * defaults to coding-task, scope to user (personal resolution). An
 * unknown kind answers 400.
 */
export async function listPromptSections(
  client: ApiClient,
  slug: string,
  options: { kind?: string; scope?: (typeof PROMPT_SCOPES)[number] } = {}
): Promise<PromptSectionListResponse> {
  const path = `/api/workspaces/${slug}/prompt-sections`;
  const body = await client.getJson<unknown>(path, { query: { ...options } });
  return client.parse(PromptSectionListResponse, body, `GET ${path}`);
}

/**
 * A stored override row. `user` is null (and `is_workspace_level` true)
 * for workspace-level rows; personal rows carry the owner's key.
 */
export const PromptSectionOverride = z.object({
  id: uuid(),
  workspace: uuid(),
  user: nullableUuid(),
  section_key: z.string(),
  body: z.string(),
  is_active: z.boolean(),
  version: z.number(),
  needs_attention: z.boolean(),
  is_workspace_level: z.boolean(),
  updated_by: nullableUuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type PromptSectionOverride = z.infer<typeof PromptSectionOverride>;

/**
 * PUT .../prompt-sections/<key> with scope plus body in the payload
 * answers 200 with the stored row (version bumped on re-save).
 * Workspace scope is admin-only; user scope is any member editing
 * their own row. Locked tiers 403 and invalid bodies 400.
 */
export async function upsertPromptSection(
  client: ApiClient,
  slug: string,
  sectionKey: string,
  input: { scope: (typeof PROMPT_SCOPES)[number]; body: string }
): Promise<PromptSectionOverride> {
  const path = `/api/workspaces/${slug}/prompt-sections/${sectionKey}`;
  const envelope = await client.sendJson<unknown>("PUT", path, { json: input });
  return client.parse(PromptSectionOverride, envelope.data, `PUT ${path}`);
}

/**
 * DELETE .../prompt-sections/<key>?scope= deactivates the override at
 * that scope (revert to the next rung) and answers 204. Reverting a
 * scope with no active override 404s.
 */
export async function revertPromptSection(
  client: ApiClient,
  slug: string,
  sectionKey: string,
  scope: (typeof PROMPT_SCOPES)[number]
): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/prompt-sections/${sectionKey}`, {
    query: { scope },
  });
  return response.status;
}

/**
 * The assembled template for a kind (Jinja markers intact). When
 * resolving at user scope for a caller with overrides, the payload
 * also carries the workspace-only template automatic runs use, so the
 * receipt can contrast the two.
 */
export const CompiledPrompt = z.object({
  kind: z.string(),
  scope: z.enum(PROMPT_SCOPES),
  template_body: z.string(),
  automatic_template_body: z.optional(z.string()),
});
export type CompiledPrompt = z.infer<typeof CompiledPrompt>;

/** GET .../prompts/<kind>/compiled?scope=. */
export async function getCompiledPrompt(
  client: ApiClient,
  slug: string,
  kind: string,
  options: { scope?: (typeof PROMPT_SCOPES)[number] } = {}
): Promise<CompiledPrompt> {
  const path = `/api/workspaces/${slug}/prompts/${kind}/compiled`;
  const body = await client.getJson<unknown>(path, { query: { ...options } });
  return client.parse(CompiledPrompt, body, `GET ${path}`);
}

/** A template rendered against real data, without creating a run. */
export const PromptPreview = z.object({
  kind: z.string(),
  prompt: z.string(),
});
export type PromptPreview = z.infer<typeof PromptPreview>;

export interface PromptPreviewInput {
  scope?: (typeof PROMPT_SCOPES)[number];
  issue_id?: string;
  binding_id?: string;
  section_key?: string;
  body?: string;
}

/**
 * POST .../prompts/<kind>/preview. Coding-task and review render
 * against `issue_id`; scheduler renders against `binding_id`. Members
 * may preview their own (user-scope) composition; the workspace
 * default stays admin-only. A `section_key` plus `body` renders an
 * unsaved draft in place of its resolved section. Render failures
 * answer 422.
 */
export async function previewPrompt(
  client: ApiClient,
  slug: string,
  kind: string,
  input: PromptPreviewInput
): Promise<PromptPreview> {
  const path = `/api/workspaces/${slug}/prompts/${kind}/preview`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(PromptPreview, envelope.data, `POST ${path}`);
}
