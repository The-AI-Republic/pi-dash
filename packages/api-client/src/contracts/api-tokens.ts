// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal access tokens: the caller's own rows. The secret appears only
// in the create answer; every read omits it and reports liveness instead.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";

/** The token row as reads answer it — never the secret. */
export const ApiToken = z.object({
  id: uuid(),
  label: z.string(),
  description: z.string(),
  is_active: z.nullable(z.boolean()),
  last_used: nullableDateTime(),
  user: uuid(),
  user_type: z.number(),
  workspace: nullableUuid(),
  expired_at: nullableDateTime(),
  is_service: z.optional(z.boolean()),
  allowed_rate_limit: z.optional(z.string()),
  created_at: dateTime(),
  updated_at: dateTime(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  deleted_at: nullableDateTime(),
});
export type ApiToken = z.infer<typeof ApiToken>;

export const ApiTokenList = z.array(ApiToken);
export type ApiTokenList = z.infer<typeof ApiTokenList>;

/**
 * The create answer. The secret rides only on this response; readers
 * must show it once and never expect it back.
 */
export const CreatedApiToken = z.extend(ApiToken, {
  token: z.string(),
});
export type CreatedApiToken = z.infer<typeof CreatedApiToken>;

export interface ApiTokenInput {
  label: string;
  description?: string;
  /** ISO timestamp, or null/omitted for a token that never expires. */
  expiredAt?: string | null;
}

/** GET /api/users/api-tokens/ — the caller's non-service tokens. */
export async function listApiTokens(client: ApiClient): Promise<ApiTokenList> {
  const body = await client.getJson<unknown>("/api/users/api-tokens/");
  return client.parse(ApiTokenList, body, "GET /api/users/api-tokens/");
}

/** POST /api/users/api-tokens/ — answers 201 with the one-time secret. */
export async function createApiToken(client: ApiClient, input: ApiTokenInput): Promise<CreatedApiToken> {
  const path = "/api/users/api-tokens/";
  const envelope = await client.sendJson<unknown>("POST", path, {
    json: {
      label: input.label,
      description: input.description,
      expired_at: input.expiredAt ?? null,
    },
  });
  return client.parse(CreatedApiToken, envelope.data, `POST ${path}`);
}

/** GET /api/users/api-tokens/<id>/ — one row, still without the secret. */
export async function getApiToken(client: ApiClient, tokenId: string): Promise<ApiToken> {
  const path = `/api/users/api-tokens/${tokenId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ApiToken, body, `GET ${path}`);
}

/** DELETE /api/users/api-tokens/<id>/ — resolves on the 204. */
export async function deleteApiToken(client: ApiClient, tokenId: string): Promise<void> {
  await client.delete(`/api/users/api-tokens/${tokenId}/`);
}
