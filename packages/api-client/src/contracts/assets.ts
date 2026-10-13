// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// User-asset upload contract: the signed-URL flow behind the onboarding
// avatar picker (AUTH-031). Request a grant, PUT the bytes to the signed
// URL, confirm the asset, then store the returned URL on the user row.
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
 * Upload one avatar image and resolve to its public URL. Three legs: grant
 * (`POST /api/assets/v2/user-assets/` with the avatar entity plus file
 * metadata), bytes (a signed POST of the grant fields plus the file), and
 * confirm (`PATCH .../{asset_id}/`). Any failed leg rejects with ApiError.
 */
export async function uploadUserAvatar(
  client: ApiClient,
  file: File,
  uploadBytes: ByteUpload = defaultByteUpload
): Promise<string> {
  const grantPath = "/api/assets/v2/user-assets/";
  const envelope = await client.sendJson<unknown>("POST", grantPath, {
    json: {
      entity_identifier: "",
      entity_type: "USER_AVATAR",
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
      message: "The avatar upload was rejected.",
    });
  }

  await client.patch(`/api/assets/v2/user-assets/${encodeURIComponent(grant.asset_id)}/`);
  return grant.asset_url;
}
