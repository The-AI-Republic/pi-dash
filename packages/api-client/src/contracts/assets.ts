// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Asset upload contracts: the signed-URL flows behind the onboarding
// avatar picker (AUTH-031), the project settings emblem control
// (PSET-007) and the workspace logo dialog (WSET-004). Request a grant,
// POST the bytes to the signed URL, confirm the asset, then store the
// returned URL on the owning row.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { ApiError } from "../errors.js";

/** Signed grant for one user-asset upload. */
export const SignedUserAsset = z.object({
  asset_id: z.string(),
  asset_url: z.string(),
  upload_data: z.object({
    url: z.string(),
    fields: z.record(z.string(), z.string()),
  }),
});
export type SignedUserAsset = z.infer<typeof SignedUserAsset>;

/** Minimal upload response the byte leg needs: ok plus status. */
export interface ByteUploadResponse {
  ok: boolean;
  status: number;
}

/**
 * PUT the file bytes to the signed URL. Defaults to the global fetch; tests
 * inject a fake. Kept outside ApiClient, whose transport only carries
 * string bodies.
 */
export type ByteUpload = (url: string, body: FormData) => Promise<ByteUploadResponse>;

async function defaultByteUpload(url: string, body: FormData): Promise<ByteUploadResponse> {
  const response = await globalThis.fetch(url, { method: "POST", body });
  return { ok: response.ok, status: response.status };
}

/**
 * One user-owned image the upload endpoint accepts: the profile avatar and
 * cover today. The project emblem control shares this route; see the note
 * on `uploadUserAsset`.
 */
export const USER_ASSET_TYPES = ["USER_AVATAR", "USER_COVER"] as const;
export type UserAssetType = (typeof USER_ASSET_TYPES)[number];

/**
 * Upload one user-owned image and resolve to its public URL. Three legs:
 * grant (`POST /api/assets/v2/user-assets/` with the entity type and file
 * metadata), bytes (a signed POST of the grant fields plus the file), and
 * confirm (`PATCH .../{asset_id}/`, which wires the asset onto the user).
 *
 * The project emblem control (PSET-007) is specified as a user-asset
 * upload; the backend's user-asset route accepts the avatar and cover
 * entity types, so the control picks the one that matches its stored
 * artwork. Any failed leg rejects with ApiError.
 */
export async function uploadUserAsset(
  client: ApiClient,
  entityType: UserAssetType,
  file: File,
  uploadBytes: ByteUpload = defaultByteUpload
): Promise<string> {
  const grantPath = "/api/assets/v2/user-assets/";
  const envelope = await client.sendJson<unknown>("POST", grantPath, {
    json: {
      entity_identifier: "",
      entity_type: entityType,
      name: file.name,
      size: file.size,
      type: file.type,
    },
  });
  const grant = client.parse(SignedUserAsset, envelope.data, `POST ${grantPath}`);

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

  await client.patch(`/api/assets/v2/user-assets/${encodeURIComponent(grant.asset_id)}/`);
  return grant.asset_url;
}

/**
 * Upload one avatar image and resolve to its public URL. Thin wrapper over
 * `uploadUserAsset` for the onboarding picker (AUTH-031).
 */
export async function uploadUserAvatar(
  client: ApiClient,
  file: File,
  uploadBytes: ByteUpload = defaultByteUpload
): Promise<string> {
  return uploadUserAsset(client, "USER_AVATAR", file, uploadBytes);
}

/**
 * Upload one workspace logo and resolve to its public URL. Three legs like
 * the avatar flow: grant (`POST /api/assets/v2/workspaces/<slug>/` with
 * the workspace-logo entity plus file metadata), bytes (a signed POST of
 * the grant fields plus the file), and confirm (`PATCH .../<asset_id>/`,
 * which also wires the asset as the workspace logo). The caller then
 * stores the returned URL on the workspace row. Any failed leg rejects
 * with ApiError.
 */
export async function uploadWorkspaceLogo(
  client: ApiClient,
  slug: string,
  workspaceId: string,
  file: File,
  uploadBytes: ByteUpload = defaultByteUpload
): Promise<string> {
  const grantPath = `/api/assets/v2/workspaces/${slug}/`;
  const envelope = await client.sendJson<unknown>("POST", grantPath, {
    json: {
      entity_identifier: workspaceId,
      entity_type: "WORKSPACE_LOGO",
      name: file.name,
      size: file.size,
      type: file.type,
    },
  });
  const grant = client.parse(SignedUserAsset, envelope.data, `POST ${grantPath}`);

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
      message: "The logo upload was rejected.",
    });
  }

  await client.patch(`/api/assets/v2/workspaces/${slug}/${encodeURIComponent(grant.asset_id)}/`);
  return grant.asset_url;
}

/**
 * DELETE /api/assets/v2/workspaces/<slug>/<asset_id>/ — drops a current
 * logo asset (204) and unwires it from the workspace. The caller then
 * PATCHes the workspace with an emptied logo field.
 */
export async function deleteWorkspaceLogoAsset(client: ApiClient, slug: string, assetId: string): Promise<number> {
  const response = await client.delete(`/api/assets/v2/workspaces/${slug}/${assetId}/`);
  return response.status;
}

/**
 * DELETE /api/workspaces/file-assets/<workspace_id>/<asset_key>/ — drops a
 * legacy full-URL logo asset (204). Removal picks this path when the stored
 * logo is a full URL, the v2 path otherwise.
 */
export async function deleteLegacyWorkspaceFileAsset(
  client: ApiClient,
  workspaceId: string,
  assetKey: string
): Promise<number> {
  const response = await client.delete(`/api/workspaces/file-assets/${workspaceId}/${assetKey}/`);
  return response.status;
}

/**
 * DELETE /api/assets/v2/user-assets/<asset_id>/ — drops one user-owned
 * image (204) and unwires it from the owning account: the server clears
 * the avatar or cover reference as a side effect. The profile avatar
 * dialog removes through this path for an asset URL.
 */
export async function deleteUserAsset(client: ApiClient, assetId: string): Promise<number> {
  const response = await client.delete(`/api/assets/v2/user-assets/${encodeURIComponent(assetId)}/`);
  return response.status;
}

/**
 * DELETE /api/users/file-assets/<asset_key>/ — drops a legacy user image
 * stored as a plain file key (204). The avatar dialog removes through
 * this path when the stored reference is not a v2 asset URL.
 */
export async function deleteLegacyUserFileAsset(client: ApiClient, assetKey: string): Promise<number> {
  const response = await client.delete(`/api/users/file-assets/${encodeURIComponent(assetKey)}/`);
  return response.status;
}
