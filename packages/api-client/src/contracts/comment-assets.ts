// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Comment image assets (ISS-197). A comment body may embed an image the
// composer uploaded before the comment existed: the upload parks the
// asset on the project, and once the comment is created the composer
// links the parked assets to it in one bulk write. The signed grant and
// byte-leg shapes are the same three-leg flow the attachment surface
// uses, under the comment entity type.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";

/** Entity type the asset service files comment images under. */
export const COMMENT_IMAGE_ENTITY_TYPE = "COMMENT_DESCRIPTION";

/** Grant for one comment-image upload. */
export const CommentImageGrant = z.object({
  asset_id: z.string(),
  asset_url: z.nullable(z.string()),
  upload_data: z.object({
    url: z.string(),
    fields: z.record(z.string(), z.string()),
  }),
});
export type CommentImageGrant = z.infer<typeof CommentImageGrant>;

/** Minimal upload response the byte leg needs: ok plus status. */
export interface CommentImageUploadResponse {
  ok: boolean;
  status: number;
}

/**
 * POST the file bytes to the signed URL. Defaults to the global fetch;
 * tests inject a fake. Kept outside ApiClient, whose transport only
 * carries string bodies.
 */
export type CommentImageByteUpload = (url: string, body: FormData) => Promise<CommentImageUploadResponse>;

async function defaultCommentImageByteUpload(url: string, body: FormData): Promise<CommentImageUploadResponse> {
  const response = await globalThis.fetch(url, { method: "POST", body });
  return { ok: response.ok, status: response.status };
}

/** First leg: park the file's metadata on the project and take the grant. */
export async function requestCommentImageUpload(
  client: ApiClient,
  slug: string,
  projectId: string,
  file: File
): Promise<CommentImageGrant> {
  const path = `/api/assets/v2/workspaces/${slug}/projects/${projectId}/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      entity_identifier: "",
      entity_type: COMMENT_IMAGE_ENTITY_TYPE,
      name: file.name,
      size: file.size,
      type: file.type,
    },
  });
  return client.parse(CommentImageGrant, envelope.data, `POST ${path}`);
}

/** Third leg: mark the parked asset uploaded. Answers 204. */
export async function completeCommentImageUpload(
  client: ApiClient,
  slug: string,
  projectId: string,
  assetId: string
): Promise<void> {
  await client.patch(`/api/assets/v2/workspaces/${slug}/projects/${projectId}/${encodeURIComponent(assetId)}/`);
}

/** One uploaded comment image: the id the bulk link needs, and its src. */
export interface UploadedCommentImage {
  assetId: string;
  /** Served address of the image (the signed grant's asset URL). */
  assetUrl: string;
}

/**
 * Upload one image end to end and resolve to its id and address. Three
 * legs: grant, bytes (a signed form POST of the grant fields plus the
 * file), and complete. The asset stays parked on the project until the
 * caller attaches it to the comment it was embedded in. Any failed leg
 * rejects with ApiError.
 */
export async function uploadCommentImage(
  client: ApiClient,
  slug: string,
  projectId: string,
  file: File,
  uploadBytes: CommentImageByteUpload = defaultCommentImageByteUpload
): Promise<UploadedCommentImage> {
  const grant = await requestCommentImageUpload(client, slug, projectId, file);

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
      message: "The image upload was rejected.",
    });
  }

  await completeCommentImageUpload(client, slug, projectId, grant.asset_id);
  return { assetId: grant.asset_id, assetUrl: grant.asset_url ?? "" };
}

/**
 * Link parked assets to the comment that embeds them. The bulk write
 * is addressed by the comment id: the service files every listed
 * comment image onto that comment. Answers 204; an empty list is a
 * no-op with no request.
 */
export async function attachCommentImages(
  client: ApiClient,
  slug: string,
  projectId: string,
  commentId: string,
  assetIds: readonly string[]
): Promise<void> {
  if (assetIds.length === 0) return;
  await client.post(`/api/assets/v2/workspaces/${slug}/projects/${projectId}/${encodeURIComponent(commentId)}/bulk/`, {
    json: { asset_ids: [...assetIds] },
  });
}
