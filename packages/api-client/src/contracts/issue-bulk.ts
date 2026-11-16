// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Bulk issue operations: one request across a selection. Bulk delete,
// bulk archive and batch dates run against the OSS backend; bulk update
// and bulk subscribe ride ungated OSS routes too (NEWFRONT-984) — the
// cloud gate lives in the web_new edition module, not the API.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ISSUE_PRIORITIES } from "./issues.js";

/**
 * The bulk-editable set, exactly: array-valued properties are appended
 * and unioned, scalars replace. Mirrors the cloud serializer's fields.
 */
export interface BulkIssueProperties {
  state_id?: string | null;
  priority?: (typeof ISSUE_PRIORITIES)[number];
  label_ids?: string[];
  assignee_ids?: string[];
  start_date?: string | null;
  target_date?: string | null;
  module_ids?: string[];
  cycle_id?: string | null;
  estimate_point?: string | null;
}

export interface BulkOperationInput {
  issue_ids: string[];
  properties: BulkIssueProperties;
}

/**
 * POST .../bulk-operation-issues/ answers 200 with the applied count.
 * Array-valued properties union onto the stored rows, scalars replace,
 * and the whole batch validates before anything is written.
 */
export const BulkOperationResult = z.object({
  message: z.string(),
  updated: z.number(),
});
export type BulkOperationResult = z.infer<typeof BulkOperationResult>;

export async function bulkUpdateIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  input: BulkOperationInput
): Promise<BulkOperationResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/bulk-operation-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(BulkOperationResult, envelope.data, `POST ${path}`);
}

/** DELETE .../bulk-delete-issues/ answers 200 {message}; admin-only. */
export const BulkDeleteResult = z.object({
  message: z.string(),
});
export type BulkDeleteResult = z.infer<typeof BulkDeleteResult>;

export async function bulkDeleteIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueIds: string[]
): Promise<BulkDeleteResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/bulk-delete-issues/`;
  const envelope = await client.sendJson<unknown>("DELETE", path, { json: { issue_ids: issueIds } });
  return client.parse(BulkDeleteResult, envelope.data, `DELETE ${path}`);
}

/**
 * POST .../bulk-archive-issues/ answers 200 with the stamped date.
 * Every issue must sit in a completed or cancelled state; one open
 * issue fails the whole batch with 400.
 */
export const BulkArchiveResult = z.object({
  archived_at: z.iso.date(),
});
export type BulkArchiveResult = z.infer<typeof BulkArchiveResult>;

export async function bulkArchiveIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueIds: string[]
): Promise<BulkArchiveResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/bulk-archive-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { issue_ids: issueIds } });
  return client.parse(BulkArchiveResult, envelope.data, `POST ${path}`);
}

/**
 * One row of a batch date update: the gantt's reschedule and resize
 * persist here. Dates are calendar strings; an omitted date keeps the
 * stored value. Ids outside the project are silently skipped.
 */
export interface BulkIssueDateUpdate {
  id: string;
  start_date?: string;
  target_date?: string;
}

/** POST .../issue-dates/ answers 200 {message} on success. */
export const BulkIssueDatesResult = z.object({
  message: z.string(),
});
export type BulkIssueDatesResult = z.infer<typeof BulkIssueDatesResult>;

/**
 * Batch issue dates: one request carries the gantt's moved bars. A row
 * whose merged start passes its target fails the whole batch with 400
 * and nothing is written.
 */
export async function bulkUpdateIssueDates(
  client: ApiClient,
  slug: string,
  projectId: string,
  updates: BulkIssueDateUpdate[]
): Promise<BulkIssueDatesResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/issue-dates/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { updates } });
  return client.parse(BulkIssueDatesResult, envelope.data, `POST ${path}`);
}

/**
 * POST .../bulk-subscribe-issues/ answers 200 with the fresh count.
 * Already-subscribed issues are skipped, not errors, so repeats are
 * safe; callers treat the resolved count as the confirmation.
 */
export const BulkSubscribeResult = z.object({
  message: z.string(),
  subscribed: z.number(),
});
export type BulkSubscribeResult = z.infer<typeof BulkSubscribeResult>;

export async function bulkSubscribeIssues(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueIds: string[]
): Promise<BulkSubscribeResult> {
  const path = `/api/workspaces/${slug}/projects/${projectId}/bulk-subscribe-issues/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { issue_ids: issueIds } });
  return client.parse(BulkSubscribeResult, envelope.data, `POST ${path}`);
}
