// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page attachment legs (parity/specs/pages.md §11.4, PAGE-048). An image
// dropped into a page body is parked on the project under the page
// description entity type, uploaded through the signed grant, confirmed,
// then linked to the page; duplication clones the asset so two pages never
// share one id. The README-style grant/confirm pair already exists for work
// item descriptions, so this module keeps the page-specific path helpers and
// the page link step next to the pages domain.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";
import { uuid } from "./fields.js";

/** Entity type the asset service files page body images under. */
export const PAGE_DESCRIPTION_ENTITY_TYPE = "PAGE_DESCRIPTION";

/** Signed grant for one page image. */
export const PageAssetGrant = z.object({
  asset_id: z.string(),
  asset_url: z.nullable(z.string()),
  upload_data: z.object({
    url: z.string(),
    fields: z.record(z.string(), z.string()),
  }),
});
export type PageAssetGrant = z.infer<typeof PageAssetGrant>;

/** Minimal upload response the byte leg needs: ok plus status. */
export interface PageAssetByteUploadResponse {
  ok: boolean;
  status: number;
}

/** POST the file bytes to the signed URL; tests inject a fake. */
export type PageAssetByteUpload = (url: string, body: FormData) => Promise<PageAssetByteUploadResponse>;

async function defaultByteUpload(url: string, body: FormData): Promise<PageAssetByteUploadResponse> {
  const response = await globalThis.fetch(url, { method: "POST", body });
  return { ok: response.ok, status: response.status };
}

/** First leg: park the image's metadata on the project and take the grant. */
export async function requestPageAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string | null,
  file: File
): Promise<PageAssetGrant> {
  const path = `/api/assets/v2/workspaces/${slug}/projects/${projectId}/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      entity_identifier: pageId ?? "",
      entity_type: PAGE_DESCRIPTION_ENTITY_TYPE,
      name: file.name,
      size: file.size,
      type: file.type,
    },
  });
  return client.parse(PageAssetGrant, envelope.data, `POST ${path}`);
}

/** Third leg: mark the parked asset uploaded. Answers 204. */
export async function completePageAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  assetId: string
): Promise<void> {
  await client.patch(`/api/assets/v2/workspaces/${slug}/projects/${projectId}/${encodeURIComponent(assetId)}/`);
}

/** One uploaded page image: the id the document stores, and its src. */
export interface UploadedPageAsset {
  assetId: string;
  assetUrl: string;
}

/** Upload one page image end to end: grant, bytes, complete. */
export async function uploadPageAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string | null,
  file: File,
  uploadBytes: PageAssetByteUpload = defaultByteUpload
): Promise<UploadedPageAsset> {
  const grant = await requestPageAsset(client, slug, projectId, pageId, file);

  const payload = new FormData();
  for (const [key, value] of Object.entries(grant.upload_data.fields)) {
    payload.append(key, value);
  }
  payload.append("file", file);
  const uploaded = await uploadBytes(grant.upload_data.url, payload);
  if (!uploaded.ok) {
    throw new ApiError({ status: uploaded.status, code: "http", message: "The image upload was rejected." });
  }

  await completePageAsset(client, slug, projectId, grant.asset_id);
  return { assetId: grant.asset_id, assetUrl: grant.asset_url ?? "" };
}

/**
 * Link parked page images to the page that embeds them. Answers 204; an
 * empty list makes no request.
 */
export async function attachPageAssets(
  client: ApiClient,
  slug: string,
  projectId: string,
  pageId: string,
  assetIds: readonly string[]
): Promise<void> {
  if (assetIds.length === 0) return;
  await client.post(`/api/assets/v2/workspaces/${slug}/projects/${projectId}/${encodeURIComponent(pageId)}/bulk/`, {
    json: { asset_ids: [...assetIds] },
  });
}

/** One cloned image: the new asset id the duplicated document references. */
export const PageAssetDuplicate = z.object({ asset_id: uuid() });
export type PageAssetDuplicate = z.infer<typeof PageAssetDuplicate>;

/** Clone one page image onto another page (duplicate never shares ids). */
export async function duplicatePageAsset(
  client: ApiClient,
  slug: string,
  projectId: string,
  sourceAssetId: string,
  pageId: string
): Promise<PageAssetDuplicate> {
  const path = `/api/assets/v2/workspaces/${slug}/duplicate-assets/${encodeURIComponent(sourceAssetId)}/`;
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: { entity_id: pageId, entity_type: PAGE_DESCRIPTION_ENTITY_TYPE, project_id: projectId },
  });
  return client.parse(PageAssetDuplicate, envelope.data, `POST ${path}`);
}

/** Whether an asset id still exists in the workspace. */
export const PageAssetExistence = z.object({ exists: z.boolean() });
export type PageAssetExistence = z.infer<typeof PageAssetExistence>;

/** Ask the asset check whether an id is live (a deleted id answers false). */
export async function checkPageAsset(client: ApiClient, slug: string, assetId: string): Promise<PageAssetExistence> {
  const path = `/api/assets/v2/workspaces/${slug}/check/${encodeURIComponent(assetId)}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(PageAssetExistence, body, `GET ${path}`);
}

/**
 * The read/thumbnail address for an asset: the project-scoped view route
 * that redirects to a signed URL. The assets pane and editor use it as the
 * image src, and the download leg appends `download/` before the id.
 */
export function pageAssetUrl(slug: string, projectId: string, assetId: string): string {
  return `/api/assets/v2/workspaces/${slug}/projects/${projectId}/${encodeURIComponent(assetId)}/`;
}

/** The attachment download address (signed URL with a disposition header). */
export function pageAssetDownloadUrl(slug: string, projectId: string, assetId: string): string {
  return `/api/assets/v2/workspaces/${slug}/projects/${projectId}/download/${encodeURIComponent(assetId)}/`;
}
