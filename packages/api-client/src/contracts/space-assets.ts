// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public asset lifecycle for the space comment composer
// (parity/specs/space.md §5.3). Same three legs as the workspace comment
// image flow, under the anchor-scoped route: park the file's metadata and
// take a signed grant, POST the bytes to that grant, confirm the asset
// (mandatory — until it runs the asset 404s), then link the confirmed
// assets to the new comment in one bulk call. A soft-deleted asset can be
// restored; the legacy editor branch uses the workspace-scoped restore and
// is not part of this area.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";

/** Entity type the space composer files comment images under. */
export const SPACE_COMMENT_IMAGE_ENTITY_TYPE = "COMMENT_DESCRIPTION";

/** Grant for one space comment-image upload. */
export const SpaceAssetGrant = z.object({
  asset_id: z.string(),
  asset_url: z.nullable(z.string()),
  upload_data: z.object({
    url: z.string(),
    fields: z.record(z.string(), z.string()),
  }),
});
export type SpaceAssetGrant = z.infer<typeof SpaceAssetGrant>;

/** Minimal byte-leg result the composer needs: ok plus status. */
export interface SpaceAssetUploadResponse {
  ok: boolean;
  status: number;
}

/**
 * POST the file bytes to the signed URL. Defaults to the global fetch;
 * tests inject a fake. Kept outside ApiClient, whose transport only
 * carries string bodies.
 */
export type SpaceAssetByteUpload = (url: string, body: FormData) => Promise<SpaceAssetUploadResponse>;

async function defaultSpaceAssetByteUpload(url: string, body: FormData): Promise<SpaceAssetUploadResponse> {
  const response = await globalThis.fetch(url, { method: "POST", body });
  return { ok: response.ok, status: response.status };
}

export interface SpaceAssetUploadInput {
  name: string;
  /** MIME type; the server allowlists jpeg/png/webp/gif. */
  type: string;
  /** Byte size. */
  size: number;
  /**
   * Comment id when editing an existing comment; empty for a new one. The
   * server stores it as the asset's comment id.
   */
  entityIdentifier?: string;
}

/** First leg: park the file's metadata and take the signed grant. */
export async function requestSpaceAssetUpload(
  client: ApiClient,
  anchor: string,
  input: SpaceAssetUploadInput
): Promise<SpaceAssetGrant> {
  const path = `/api/public/assets/v2/anchor/${encodeURIComponent(anchor)}/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      name: input.name,
      type: input.type,
      size: input.size,
      entity_type: SPACE_COMMENT_IMAGE_ENTITY_TYPE,
      entity_identifier: input.entityIdentifier ?? "",
    },
  });
  return client.parse(SpaceAssetGrant, envelope.data, `POST ${path}`);
}

/**
 * Second leg: POST the bytes to the signed storage URL. Pass the grant's
 * `upload_data.url` and a form seeded with `upload_data.fields` plus the
 * `file` part.
 */
export async function uploadSpaceAssetBytes(
  grant: SpaceAssetGrant,
  form: FormData,
  upload: SpaceAssetByteUpload = defaultSpaceAssetByteUpload
): Promise<SpaceAssetUploadResponse> {
  return upload(grant.upload_data.url, form);
}

/** Third leg (required): mark the parked asset uploaded. Answers 204. */
export async function completeSpaceAssetUpload(client: ApiClient, anchor: string, assetId: string): Promise<void> {
  await client.patch(`/api/public/assets/v2/anchor/${encodeURIComponent(anchor)}/${encodeURIComponent(assetId)}/`, {
    json: {},
  });
}

/** Links the uploaded assets to a newly created comment. Answers 204. */
export async function linkSpaceAssets(
  client: ApiClient,
  anchor: string,
  entityId: string,
  assetIds: string[]
): Promise<void> {
  await client.post(
    `/api/public/assets/v2/anchor/${encodeURIComponent(anchor)}/${encodeURIComponent(entityId)}/bulk/`,
    { json: { asset_ids: assetIds } }
  );
}

/** Restores a soft-deleted asset (editor lifecycle). Answers 204. */
export async function restoreSpaceAsset(client: ApiClient, anchor: string, assetId: string): Promise<void> {
  await client.post(
    `/api/public/assets/v2/anchor/${encodeURIComponent(anchor)}/restore/${encodeURIComponent(assetId)}/`,
    { json: {} }
  );
}

/**
 * Read address for an asset image: the public route redirects to a signed
 * storage URL and 404s until the asset is confirmed. Returned as a path so
 * the caller resolves it against its API origin.
 */
export function spaceAssetReadPath(anchor: string, assetId: string): string {
  return `/api/public/assets/v2/anchor/${encodeURIComponent(anchor)}/${encodeURIComponent(assetId)}/`;
}
