// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace issue-export contracts: the create call that queues a file
// build and the cursor-paged history read behind previous exports. The
// profile activity download lives in profile-activity.ts, not here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableUuid, uuid } from "./fields.js";
import { UserLite } from "./users.js";

/** Provider ids the server accepts on create. Anything else answers a 400. */
export const EXPORT_PROVIDERS = ["csv", "xlsx", "json"] as const;
export type ExportProvider = (typeof EXPORT_PROVIDERS)[number];
export const ExportProvider = z.enum(EXPORT_PROVIDERS);

/** Lifecycle states the server stores on an export row. */
export const EXPORT_STATUSES = ["queued", "processing", "completed", "failed"] as const;
export type ExportStatus = (typeof EXPORT_STATUSES)[number];
export const ExportStatus = z.enum(EXPORT_STATUSES);

/**
 * Whether a stored provider id is one of the three accepted ids. Stored
 * rows keep raw strings so a future provider never breaks the parse;
 * callers render the unknown marker when this answers false.
 */
export function isKnownExportProvider(value: string): value is ExportProvider {
  return (EXPORT_PROVIDERS as readonly string[]).includes(value);
}

/**
 * Whether a stored status is one of the four lifecycle states. Unknown
 * strings parse through and render the unknown marker instead of the
 * state pill. The server never stores an expired status.
 */
export function isKnownExportStatus(value: string): value is ExportStatus {
  return (EXPORT_STATUSES as readonly string[]).includes(value);
}

/**
 * One history row. `project` is null when the export covers no stored
 * projects — callers read that as an empty list for the count. `url`
 * stays null until the file build finishes. `provider` and `status`
 * are raw strings with the `isKnown*` guards above, so unknown values
 * parse and only change how the row renders.
 */
export const WorkspaceExport = z.object({
  id: uuid(),
  created_at: dateTime(),
  updated_at: dateTime(),
  project: z.nullable(z.array(uuid())),
  provider: z.string(),
  status: z.string(),
  url: z.nullable(z.string()),
  initiated_by: uuid(),
  initiated_by_detail: UserLite,
  token: z.string(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
});
export type WorkspaceExport = z.infer<typeof WorkspaceExport>;

/**
 * Cursor envelope for the history read. `next_page_results` and
 * `prev_page_results` are the end-of-list flags the pager follows;
 * callers never infer the ends from `results.length`.
 */
export const WorkspaceExportsPage = z.object({
  grouped_by: z.nullable(z.string()),
  sub_grouped_by: z.nullable(z.string()),
  total_count: z.number(),
  next_cursor: z.string(),
  prev_cursor: z.string(),
  next_page_results: z.boolean(),
  prev_page_results: z.boolean(),
  count: z.number(),
  total_pages: z.number(),
  total_results: z.number(),
  extra_stats: z.nullable(z.unknown()),
  results: z.array(WorkspaceExport),
});
export type WorkspaceExportsPage = z.infer<typeof WorkspaceExportsPage>;

export interface WorkspaceExportsQuery {
  /**
   * Rows per page. The server requires this together with `cursor`
   * (400 otherwise); the history's first read asks for 10.
   */
  perPage?: number;
  /**
   * `limit:page:is_prev` cursor. The history's first read sends
   * `10:0:0`, then follows the envelope's cursors.
   */
  cursor?: string;
  /** Sort key; the server defaults to `-created_at`. */
  orderBy?: string;
}

function exportsParams(query: WorkspaceExportsQuery) {
  return {
    per_page: query.perPage,
    cursor: query.cursor,
    order_by: query.orderBy,
  };
}

/**
 * GET /api/workspaces/<slug>/export-issues (cursor-paged; the read
 * carries no trailing slash). Missing `per_page` or `cursor` answers
 * a 400 whose `{ error }` surfaces as `ApiError.message`.
 */
export async function listWorkspaceExports(
  client: ApiClient,
  slug: string,
  query: WorkspaceExportsQuery = {}
): Promise<WorkspaceExportsPage> {
  const path = `/api/workspaces/${slug}/export-issues`;
  const body = await client.getJson<unknown>(path, { query: exportsParams(query) });
  return client.parse(WorkspaceExportsPage, body, `GET ${path}`);
}

export interface CreateWorkspaceExportInput {
  /** File format; only the three accepted ids may be sent. */
  provider: ExportProvider;
  /** Project ids; empty expands to the caller's active memberships. */
  project: string[];
}

/** POST success acknowledgement; the file build runs in the background. */
export const WorkspaceExportReceipt = z.object({
  message: z.string(),
});
export type WorkspaceExportReceipt = z.infer<typeof WorkspaceExportReceipt>;

/**
 * POST /api/workspaces/<slug>/export-issues/ with the provider and
 * project ids. `multiple` (separate file per project) derives from
 * the project count — more than one project means separate files —
 * so callers never compute it themselves. An unknown provider answers
 * a 400 whose `{ error }` surfaces as `ApiError.message`.
 */
export async function createWorkspaceExport(
  client: ApiClient,
  slug: string,
  input: CreateWorkspaceExportInput
): Promise<WorkspaceExportReceipt> {
  const path = `/api/workspaces/${slug}/export-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { provider: input.provider, project: input.project, multiple: input.project.length > 1 },
  });
  return client.parse(WorkspaceExportReceipt, envelope.data, `POST ${path}`);
}
