// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal access tokens: the caller's own rows. The secret appears only
// in the create answer; every read omits it and reports liveness instead.
// Personal API-token contracts (WSET-025-028): the signed-in user's own
// token list, creation with a one-time raw secret, and deletion. These are
// user-scoped, not workspace-scoped.
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

/**
 * One token row. Reads never include the raw secret; `is_active` is the
 * server's liveness answer for the expiry (null expiry is always live).
 */
export const PersonalApiToken = z.object({
  id: uuid(),
  is_active: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
  label: z.string(),
  description: z.string(),
  last_used: nullableDateTime(),
  user_type: z.number(),
  expired_at: nullableDateTime(),
  is_service: z.boolean(),
  allowed_rate_limit: z.string(),
  created_by: nullableUuid(),
  updated_by: nullableUuid(),
  user: uuid(),
  workspace: nullableUuid(),
});
export type PersonalApiToken = z.infer<typeof PersonalApiToken>;

export const PersonalApiTokenList = z.array(PersonalApiToken);
export type PersonalApiTokenList = z.infer<typeof PersonalApiTokenList>;

/**
 * GET /api/users/api-tokens/ — the caller's own non-service tokens,
 * newest first. Each row carries the liveness flag the UI's marker reads.
 */
export async function listPersonalApiTokens(client: ApiClient): Promise<PersonalApiTokenList> {
  const path = "/api/users/api-tokens/";
  const body = await client.getJson<unknown>(path);
  return client.parse(PersonalApiTokenList, body, `GET ${path}`);
}

export interface CreatePersonalApiTokenInput {
  label: string;
  description: string;
  /** ISO datetime; null means the token never expires. */
  expired_at: string | null;
}

/**
 * The create answer: the stored row plus the raw secret. This is the only
 * time the secret is returned; the row's read projection drops it.
 */
export const CreatedPersonalApiToken = z.extend(PersonalApiToken, {
  token: z.string(),
});
export type CreatedPersonalApiToken = z.infer<typeof CreatedPersonalApiToken>;

/**
 * POST /api/users/api-tokens/ — create with the label, description and
 * expiry (or null for never). Answers 201 with the raw secret; the caller
 * shows it once and discards it.
 */
export async function createPersonalApiToken(
  client: ApiClient,
  input: CreatePersonalApiTokenInput
): Promise<CreatedPersonalApiToken> {
  const path = "/api/users/api-tokens/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(CreatedPersonalApiToken, envelope.data, `POST ${path}`);
}

/**
 * DELETE /api/users/api-tokens/<tokenId> — permanent removal (204). The
 * route's item path is declared with a trailing slash server-side; this
 * contract pins the slashless address the spec names so the slice asks for
 * exactly one address.
 */
export async function deletePersonalApiToken(client: ApiClient, tokenId: string): Promise<number> {
  const response = await client.delete(`/api/users/api-tokens/${tokenId}`);
  return response.status;
}
