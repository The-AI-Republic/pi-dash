// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner LLM model catalog contracts (NEWPIDASH-169, DIFF-8): the
// signed-in picker read plus the instance-admin CRUD the god-mode page
// drives. The server groups rows by agent; every agent key is present,
// empty when it has no rows in the filtered set.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, uuid } from "./fields.js";

/** One catalog row as the read/CRUD endpoints report it. */
export const RunnerModelRow = z.object({
  id: uuid(),
  agent: z.string(),
  model: z.string(),
  label: z.string(),
  reasoning_efforts: z.array(z.string()),
  sort_order: z.number(),
  is_enabled: z.boolean(),
  is_preselected: z.boolean(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type RunnerModelRow = z.infer<typeof RunnerModelRow>;

/** Grouped catalog: every agent key, rows in picker order. */
export const RunnerModelCatalogResponse = z.object({
  agents: z.record(z.string(), z.array(RunnerModelRow)),
});
export type RunnerModelCatalogResponse = z.infer<typeof RunnerModelCatalogResponse>;

/**
 * Any signed-in user reads the enabled models for the picker.
 * The add-runner form falls back to its built-in list when this
 * call fails, so errors surface upstream as ApiErrors.
 */
export async function getModelCatalog(client: ApiClient): Promise<RunnerModelCatalogResponse> {
  const path = "/api/runners/model-catalog/";
  const body = await client.getJson<unknown>(path);
  return client.parse(RunnerModelCatalogResponse, body, `GET ${path}`);
}

/** Instance admins list every row, including disabled ones. */
export async function listAdminRunnerModels(client: ApiClient): Promise<RunnerModelCatalogResponse> {
  const path = "/api/instances/runner-models/";
  const body = await client.getJson<unknown>(path);
  return client.parse(RunnerModelCatalogResponse, body, `GET ${path}`);
}

export interface RunnerModelCreateInput {
  agent: string;
  model: string;
  label: string;
  reasoning_efforts?: string[];
  sort_order?: number;
  is_enabled?: boolean;
  is_preselected?: boolean;
}

/** Instance admins add a model; `201` answers the created row. */
export async function createRunnerModel(client: ApiClient, input: RunnerModelCreateInput): Promise<RunnerModelRow> {
  const path = "/api/instances/runner-models/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: input });
  return client.parse(RunnerModelRow, envelope.data, `POST ${path}`);
}

export interface RunnerModelPatchInput {
  model?: string;
  label?: string;
  reasoning_efforts?: string[];
  sort_order?: number;
  is_enabled?: boolean;
  is_preselected?: boolean;
}

/** Instance admins edit a model (agent is never writable). */
export async function updateRunnerModel(
  client: ApiClient,
  rowId: string,
  input: RunnerModelPatchInput
): Promise<RunnerModelRow> {
  const path = `/api/instances/runner-models/${rowId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: input });
  return client.parse(RunnerModelRow, envelope.data, `PATCH ${path}`);
}

/** Instance admins delete a model (`204` answers empty). */
export async function deleteRunnerModel(client: ApiClient, rowId: string): Promise<number> {
  const response = await client.delete(`/api/instances/runner-models/${rowId}/`);
  return response.status;
}
