// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-surface model choice (NEWPIDASH-394, design section 6.1): each user
// keeps a separate saved choice per surface (web_assistant,
// desktop_assistant, agent); the server answers the saved entry, the
// effective choice it resolves to, and why they differ. Also the per-ID
// smoke-test results every BYOK write returns (design sections 6.3, 6.4).
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { isApiError } from "../errors.js";
import { nullableDateTime, uuid } from "./fields.js";

/**
 * The surfaces a saved choice names. Which surface a turn ran on is
 * stamped by the server; this union only names the endpoint slot and the
 * surface a Settings picker or the composer writes.
 */
export const ASSISTANT_SURFACES = ["web_assistant", "desktop_assistant", "agent"] as const;
export type AssistantSurface = (typeof ASSISTANT_SURFACES)[number];

/**
 * One saved or effective choice. `connection_id` names the BYOK
 * connection whose key runs the model; catalog (OpenHub) choices carry
 * no connection.
 */
export const ModelSelectionChoice = z.object({
  model: z.string(),
  connection_id: z.optional(z.nullable(uuid())),
});
export type ModelSelectionChoice = z.infer<typeof ModelSelectionChoice>;

/**
 * One surface's selection state. `fallback_reason` is the server's
 * machine code (`none_saved`, `model_removed`, `connection_removed`,
 * `model_disabled`, `plan_blocked`, `lane_unavailable`,
 * `not_allowed_for_surface`); absent when the saved entry applies.
 */
export const ModelSelectionSurfaceState = z.object({
  saved: z.nullable(ModelSelectionChoice),
  effective: z.nullable(ModelSelectionChoice),
  fallback_reason: z.optional(z.nullable(z.string())),
});
export type ModelSelectionSurfaceState = z.infer<typeof ModelSelectionSurfaceState>;

/** The whole selection answer: one state per surface. */
export const ModelSelection = z.object({
  surfaces: z.object({
    web_assistant: ModelSelectionSurfaceState,
    desktop_assistant: ModelSelectionSurfaceState,
    agent: ModelSelectionSurfaceState,
  }),
});
export type ModelSelection = z.infer<typeof ModelSelection>;

/** Read every surface's saved and effective choice. */
export async function getModelSelection(client: ApiClient): Promise<ModelSelection> {
  const body = await client.getJson<unknown>("/api/users/me/ai-assistant/model-selection/");
  return client.parse(ModelSelection, body, "GET /api/users/me/ai-assistant/model-selection/");
}

export interface ModelSelectionPutInput {
  model: string;
  /** The BYOK connection; omit for a catalog (OpenHub) model. */
  connectionId?: string | null;
}

/**
 * Save one surface's choice. Answers the surface's state after the
 * write. Refusals (`unknown_surface`, `model_not_available`,
 * `connection_not_found`, `not_allowed_for_surface`, `plan_blocked`)
 * ride the standard assistant error envelope — read them with
 * `assistantErrorFrom`.
 */
export async function putModelSelection(
  client: ApiClient,
  surface: AssistantSurface,
  input: ModelSelectionPutInput
): Promise<ModelSelectionSurfaceState> {
  const path = `/api/users/me/ai-assistant/model-selection/${surface}/`;
  const envelope = await client.sendJson<unknown>("PUT", path, {
    json: { model: input.model, connection_id: input.connectionId ?? null },
  });
  return client.parse(ModelSelectionSurfaceState, envelope.data, `PUT ${path}`);
}

/** Clear one surface's choice; the surface falls back to the default. */
export async function deleteModelSelection(client: ApiClient, surface: AssistantSurface): Promise<void> {
  await client.delete(`/api/users/me/ai-assistant/model-selection/${surface}/`);
}

/** Save-time smoke-test verdicts for one model ID. */
export const MODEL_TEST_STATUSES = ["verified", "unverified", "failed"] as const;
export type ModelTestStatus = (typeof MODEL_TEST_STATUSES)[number];

/**
 * One ID's smoke-test result. `code` is the probe's machine code
 * (`model_invalid`, `provider_auth_failed`, `provider_unreachable`,
 * ...) with the provider's message in `detail`; both are absent on a
 * verified ID.
 */
export const ModelTestResult = z.object({
  model: z.string(),
  status: z.enum(MODEL_TEST_STATUSES),
  code: z.optional(z.string()),
  detail: z.optional(z.string()),
});
export type ModelTestResult = z.infer<typeof ModelTestResult>;

/** The per-ID results envelope every BYOK write returns. */
export const ModelTestResults = z.object({
  models: z.array(ModelTestResult),
});
export type ModelTestResults = z.infer<typeof ModelTestResults>;

/**
 * Read the per-ID results off a rejected BYOK write: the `models` list
 * from a `400 model_test_failed` answer. The refusal may carry the
 * machine code alongside the results (`{error, detail, models}`) or
 * answer the bare per-ID envelope (`{models}`); both read. Null for any
 * other failure (transport errors, contract violations, a different
 * machine code, or another error shape).
 */
export function modelTestResultsFrom(error: unknown): ModelTestResults | null {
  if (!isApiError(error)) return null;
  const body = error.body;
  if (typeof body !== "object" || body === null || Array.isArray(body)) return null;
  if ("error" in body && (body as { error: unknown }).error !== "model_test_failed") return null;
  const parsed = ModelTestResults.safeParse(body);
  return parsed.success ? parsed.data : null;
}

/**
 * Test one stored model ID (`POST .../config/test/` on OSS, and the new
 * `POST .../connections/{id}/test/` on cloud, design section 6.4), both
 * accepting `{ "model" }` and answering one result object. Updates the
 * ID's `model_verified_at` server-side.
 */
export async function testAssistantModel(client: ApiClient, path: string, model: string): Promise<ModelTestResult> {
  const envelope = await client.sendJson<unknown>("POST", path, { json: { model } });
  return client.parse(ModelTestResult, envelope.data, `POST ${path}`);
}

/** Per-ID verification stamps: model ID to the instant it last passed. */
export function modelVerifiedAtMap() {
  return z.optional(z.record(z.string(), nullableDateTime()));
}
