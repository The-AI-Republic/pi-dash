// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attachment contracts: the signed three-leg upload (request a grant,
// put the bytes to the signed URL, mark the asset complete), the
// uploaded-only list, and the soft delete. Opening a file is the
// served asset URL in a new tab — the endpoint answers a redirect to
// a presigned object URL, proven in the contract suite — so there is
// no download call here. The instance file-size limit rides on the
// shared instance config (see auth.ts).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** One uploaded attachment as the list and the grant carry it. */
export const IssueAttachment = z.object({
  id: uuid(),
  attributes: z.record(z.string(), z.unknown()),
  asset: z.string(),
  user: nullableUuid(),
  workspace: nullableUuid(),
  draft_issue: nullableUuid(),
  project: nullableUuid(),
  issue: nullableUuid(),
  comment: nullableUuid(),
  page: nullableUuid(),
  entity_type: z.nullable(z.string()),
  entity_identifier: z.nullable(z.string()),
  is_deleted: z.boolean(),
  is_archived: z.boolean(),
  external_id: z.nullable(z.string()),
  external_source: z.nullable(z.string()),
  size: z.number(),
  is_uploaded: z.boolean(),
  storage_metadata: z.nullable(z.record(z.string(), z.unknown())),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
  asset_url: z.nullable(z.string()),
});
export type IssueAttachment = z.infer<typeof IssueAttachment>;

export const IssueAttachmentList = z.array(IssueAttachment);
export type IssueAttachmentList = z.infer<typeof IssueAttachmentList>;

/** The signed grant the metadata POST answers. */
export const AttachmentUploadGrant = z.object({
  upload_data: z.object({
    url: z.string(),
    fields: z.record(z.string(), z.string()),
  }),
  asset_id: z.string(),
  attachment: IssueAttachment,
  asset_url: z.nullable(z.string()),
});
export type AttachmentUploadGrant = z.infer<typeof AttachmentUploadGrant>;

export interface AttachmentUploadInput {
  name: string;
  type: string;
  size: number;
}

export async function listIssueAttachments(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string
): Promise<IssueAttachmentList> {
  const path = `/api/assets/v2/workspaces/${slug}/projects/${projectId}/issues/${issueId}/attachments/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(IssueAttachmentList, body, `GET ${path}`);
}

/**
 * First upload leg: post the file metadata and receive the signed
 * grant. The server caps the size at the instance limit and rejects
 * MIME types outside its allowlist with a 400.
 */
export async function requestAttachmentUpload(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  input: AttachmentUploadInput
): Promise<AttachmentUploadGrant> {
  const path = `/api/assets/v2/workspaces/${slug}/projects/${projectId}/issues/${issueId}/attachments/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(AttachmentUploadGrant, envelope.data, `POST ${path}`);
}

/**
 * Third upload leg: mark the asset complete once its bytes have
 * landed. Answers 204; the row then shows in the list.
 */
export async function completeAttachmentUpload(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  assetId: string
): Promise<void> {
  await client.patch(
    `/api/assets/v2/workspaces/${slug}/projects/${projectId}/issues/${issueId}/attachments/${assetId}/`
  );
}

export async function deleteIssueAttachment(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  assetId: string
): Promise<void> {
  await client.delete(
    `/api/assets/v2/workspaces/${slug}/projects/${projectId}/issues/${issueId}/attachments/${assetId}/`
  );
}

/** Minimal upload response the byte leg needs: ok plus status. */
export interface AttachmentByteUploadResponse {
  ok: boolean;
  status: number;
}

/**
 * PUT the file bytes to the signed URL. Defaults to a signed form
 * POST; tests inject a fake. Kept outside ApiClient, whose transport
 * only carries string bodies.
 */
export type AttachmentByteUpload = (url: string, body: FormData) => Promise<AttachmentByteUploadResponse>;

async function defaultAttachmentByteUpload(url: string, body: FormData): Promise<AttachmentByteUploadResponse> {
  const response = await globalThis.fetch(url, { method: "POST", body });
  return { ok: response.ok, status: response.status };
}

/**
 * Upload one file end to end and resolve to its attachment row. Three
 * legs: grant (metadata POST), bytes (a signed POST of the grant
 * fields plus the file), and complete (the asset PATCH). The complete
 * leg answers 204 with no row, so the resolved row is the grant
 * snapshot with the upload flip applied; widgets that need
 * server-fresh timestamps re-list. Any failed leg rejects with
 * ApiError.
 */
export async function uploadIssueAttachment(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  file: File,
  uploadBytes: AttachmentByteUpload = defaultAttachmentByteUpload
): Promise<IssueAttachment> {
  const grant = await requestAttachmentUpload(client, slug, projectId, issueId, {
    name: file.name,
    type: file.type,
    size: file.size,
  });

  const payload = new FormData();
  for (const [key, value] of Object.entries(grant.upload_data.fields)) {
    payload.append(key, value);
  }
  payload.append("file", file);
  const uploaded = await uploadBytes(grant.upload_data.url, payload);
  if (!uploaded.ok) {
    throw new ApiError({
      status: uploaded.status,
      code: "http",
      message: "The attachment upload was rejected.",
    });
  }

  await completeAttachmentUpload(client, slug, projectId, issueId, grant.asset_id);
  return { ...grant.attachment, is_uploaded: true };
}
