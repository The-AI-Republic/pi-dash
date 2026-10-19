// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace webhook contracts (WSET-019-024, WSET-041): the list and its
// row switch, creation, the detail form, deletion, and secret regeneration.
// The one-time secret appears only in the create and regenerate answers; it
// never comes back from a read.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, uuid } from "./fields.js";

/**
 * A webhook row as the list and detail reads answer it: metadata and the
 * five event flags only. Reads must never expose the secret (spec §§18-22);
 * the one-time reveal comes from the create and regenerate answers. The
 * backend's read serializer currently still echoes `secret_key`, which a
 * strict object drops, so a leaked field can never reach a slice's UI.
 */
export const WorkspaceWebhook = z.object({
  id: uuid(),
  url: z.string(),
  is_active: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
  project: z.boolean(),
  issue: z.boolean(),
  cycle: z.boolean(),
  module: z.boolean(),
  issue_comment: z.boolean(),
});
export type WorkspaceWebhook = z.infer<typeof WorkspaceWebhook>;

export const WorkspaceWebhookList = z.array(WorkspaceWebhook);
export type WorkspaceWebhookList = z.infer<typeof WorkspaceWebhookList>;

/** GET /api/workspaces/<slug>/webhooks/ — the admin-only list, newest first. */
export async function listWorkspaceWebhooks(client: ApiClient, slug: string): Promise<WorkspaceWebhookList> {
  const path = `/api/workspaces/${slug}/webhooks/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceWebhookList, body, `GET ${path}`);
}

/** GET /api/workspaces/<slug>/webhooks/<webhookId>/ — the detail form's read. */
export async function getWorkspaceWebhook(
  client: ApiClient,
  slug: string,
  webhookId: string
): Promise<WorkspaceWebhook> {
  const path = `/api/workspaces/${slug}/webhooks/${webhookId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(WorkspaceWebhook, body, `GET ${path}`);
}

export interface CreateWorkspaceWebhookInput {
  url: string;
  project: boolean;
  issue: boolean;
  cycle: boolean;
  module: boolean;
  issue_comment: boolean;
}

/**
 * The create answer: the stored row plus its freshly generated secret. The
 * secret is shown once and never returned by a read afterwards. A duplicate
 * target URL for the workspace answers 409 before this shape is parsed.
 */
export const CreatedWorkspaceWebhook = z.object({
  id: uuid(),
  url: z.string(),
  is_active: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
  project: z.boolean(),
  issue: z.boolean(),
  cycle: z.boolean(),
  module: z.boolean(),
  issue_comment: z.boolean(),
  secret_key: z.string(),
});
export type CreatedWorkspaceWebhook = z.infer<typeof CreatedWorkspaceWebhook>;

/**
 * POST /api/workspaces/<slug>/webhooks/ — create with the URL and all five
 * event flags. The API's URL check resolves the host and refuses private,
 * loopback, reserved, and the instance's own domains, so a rejected target
 * answers 400 with a field error.
 */
export async function createWorkspaceWebhook(
  client: ApiClient,
  slug: string,
  input: CreateWorkspaceWebhookInput
): Promise<CreatedWorkspaceWebhook> {
  const path = `/api/workspaces/${slug}/webhooks/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(CreatedWorkspaceWebhook, envelope.data, `POST ${path}`);
}

export interface UpdateWorkspaceWebhookInput {
  url?: string;
  is_active?: boolean;
  project?: boolean;
  issue?: boolean;
  cycle?: boolean;
  module?: boolean;
  issue_comment?: boolean;
}

/**
 * PATCH /api/workspaces/<slug>/webhooks/<webhookId>/ — the detail form's
 * save and the list row's active switch (which sends `is_active` alone).
 * Answers the stored row without the secret.
 */
export async function updateWorkspaceWebhook(
  client: ApiClient,
  slug: string,
  webhookId: string,
  input: UpdateWorkspaceWebhookInput
): Promise<WorkspaceWebhook> {
  const path = `/api/workspaces/${slug}/webhooks/${webhookId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(WorkspaceWebhook, envelope.data, `PATCH ${path}`);
}

/** DELETE /api/workspaces/<slug>/webhooks/<webhookId>/ — 204; later reads 404. */
export async function deleteWorkspaceWebhook(client: ApiClient, slug: string, webhookId: string): Promise<number> {
  const response = await client.delete(`/api/workspaces/${slug}/webhooks/${webhookId}/`);
  return response.status;
}

/**
 * POST /api/workspaces/<slug>/webhooks/<webhookId>/regenerate/ with no body.
 * Issues a fresh secret and answers the row plus that secret once; the
 * previous key stops authenticating deliveries.
 */
export async function regenerateWorkspaceWebhookSecret(
  client: ApiClient,
  slug: string,
  webhookId: string
): Promise<CreatedWorkspaceWebhook> {
  const path = `/api/workspaces/${slug}/webhooks/${webhookId}/regenerate/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: {} });
  return client.parse(CreatedWorkspaceWebhook, envelope.data, `POST ${path}`);
}

/** All-events mode: every delivery kind is on. */
export function allWebhookEvents(): Omit<CreateWorkspaceWebhookInput, "url"> {
  return { project: true, issue: true, cycle: true, module: true, issue_comment: true };
}

/** A row uses all-events mode exactly when every flag is set. */
export function webhookUsesAllEvents(webhook: WorkspaceWebhook): boolean {
  return webhook.project && webhook.issue && webhook.cycle && webhook.module && webhook.issue_comment;
}
