// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item description assets (parity/specs/issues.md §11, ISS-127).
// An image dropped into the modal's description is parked on the project
// under the description entity type, served from the signed grant's
// address, and linked to the work item once it exists (or right away for
// an edit). Drafts use their own entity type, so a promoted draft's
// images already belong to the row that becomes the issue.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";

/** Entity type the asset service files work-item description images under. */
export const ISSUE_DESCRIPTION_ENTITY_TYPE = "ISSUE_DESCRIPTION";
/** Entity type for a draft's description images. */
export const DRAFT_DESCRIPTION_ENTITY_TYPE = "DRAFT_ISSUE_DESCRIPTION";
export type DescriptionAssetEntityType = typeof ISSUE_DESCRIPTION_ENTITY_TYPE | typeof DRAFT_DESCRIPTION_ENTITY_TYPE;

/** The description an upload belongs to, before or after it exists. */
export interface DescriptionAssetScope {
  entityType: DescriptionAssetEntityType;
  /** Row id for an edit; empty for a not-yet-created item. */
  entityId: string;
}

/** Grant for one description image. */
export const DescriptionAssetGrant = z.object({
  asset_id: z.string(),
  asset_url: z.nullable(z.string()),
  upload_data: z.object({
    url: z.string(),
    fields: z.record(z.string(), z.string()),
  }),
});
export type DescriptionAssetGrant = z.infer<typeof DescriptionAssetGrant>;

/** Minimal upload response the byte leg needs: ok plus status. */
export interface DescriptionByteUploadResponse {
  ok: boolean;
  status: number;
}

/** POST the file bytes to the signed URL; tests inject a fake. */
export type DescriptionByteUpload = (url: string, body: FormData) => Promise<DescriptionByteUploadResponse>;

async function defaultByteUpload(url: string, body: FormData): Promise<DescriptionByteUploadResponse> {
  const response = await globalThis.fetch(url, { method: "POST", body });
  return { ok: response.ok, status: response.status };
}

/** First leg: park the file's metadata on the project and take the grant. */
export async function requestDescriptionAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  scope: DescriptionAssetScope,
  file: File
): Promise<DescriptionAssetGrant> {
  const path = `/api/assets/v2/workspaces/${slug}/projects/${projectId}/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      entity_identifier: scope.entityId,
      entity_type: scope.entityType,
      name: file.name,
      size: file.size,
      type: file.type,
    },
  });
  return client.parse(DescriptionAssetGrant, envelope.data, `POST ${path}`);
}

/** Third leg: mark the parked asset uploaded. Answers 204. */
export async function completeDescriptionAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  assetId: string
): Promise<void> {
  await client.patch(`/api/assets/v2/workspaces/${slug}/projects/${projectId}/${encodeURIComponent(assetId)}/`);
}

/** One uploaded description image: the id the link needs, and its src. */
export interface UploadedDescriptionAsset {
  assetId: string;
  assetUrl: string;
}

/**
 * Upload one description image end to end: grant, bytes, complete. The
 * asset stays parked until the caller links it to the work item.
 */
export async function uploadDescriptionAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  scope: DescriptionAssetScope,
  file: File,
  uploadBytes: DescriptionByteUpload = defaultByteUpload
): Promise<UploadedDescriptionAsset> {
  const grant = await requestDescriptionAsset(client, slug, projectId, scope, file);

  const payload = new FormData();
  for (const [key, value] of Object.entries(grant.upload_data.fields)) {
    payload.append(key, value);
  }
  payload.append("file", file);
  const uploaded = await uploadBytes(grant.upload_data.url, payload);
  if (!uploaded.ok) {
    throw new ApiError({ status: uploaded.status, code: "http", message: "The image upload was rejected." });
  }

  await completeDescriptionAsset(client, slug, projectId, grant.asset_id);
  return { assetId: grant.asset_id, assetUrl: grant.asset_url ?? "" };
}

/**
 * Link parked description assets to the work item that embeds them.
 * Answers 204; an empty list makes no request.
 */
export async function attachDescriptionAssets(
  client: ApiClient,
  slug: string,
  projectId: string,
  issueId: string,
  assetIds: readonly string[]
): Promise<void> {
  if (assetIds.length === 0) return;
  await client.post(`/api/assets/v2/workspaces/${slug}/projects/${projectId}/${encodeURIComponent(issueId)}/bulk/`, {
    json: { asset_ids: [...assetIds] },
  });
}
