// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner LLM model catalog contracts (PDASHOSS01-364): the picker source
// plus the instance-admin CRUD. Mirrors the Django contract byte-for-byte:
// the public read groups by agent; the admin list is the flat ordered list.

import * as z from "zod/mini";

import { ApiClient } from "../client.js";
import { dateTime, uuid } from "./fields.js";

export const RUNNER_CATALOG_AGENTS = [
  "claude-code",
  "codex",
  "cursor-agent",
  "open-claw",
  "grok",
  "muse-code",
] as const;
export type RunnerCatalogAgent = (typeof RUNNER_CATALOG_AGENTS)[number];

export const RUNNER_CATALOG_EFFORTS = ["low", "medium", "high", "xhigh"] as const;
export type RunnerCatalogEffort = (typeof RUNNER_CATALOG_EFFORTS)[number];

export const RunnerCatalogRow = z.object({
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
export type RunnerCatalogRow = z.infer<typeof RunnerCatalogRow>;

export const RunnerCatalogResponse = z.object({
  agents: z.record(z.string(), z.array(RunnerCatalogRow)),
});
export type RunnerCatalogResponse = z.infer<typeof RunnerCatalogResponse>;

/**
 * Picker source: enabled models grouped by agent, every agent key present,
 * sorted by sort_order/created_at. Any signed-in user.
 */
export async function getRunnerCatalog(client: ApiClient): Promise<RunnerCatalogResponse> {
  const path = "/api/runners/model-catalog/";
  const body = await client.getJson<unknown>(path);
  return client.parse(RunnerCatalogResponse, body, `GET ${path}`);
}

export const RunnerModelCatalogList = z.array(RunnerCatalogRow);
export type RunnerModelCatalogList = z.infer<typeof RunnerModelCatalogList>;

export interface RunnerModelWrite {
  agent?: string;
  model?: string;
  label?: string;
  reasoning_efforts?: string[];
  sort_order?: number;
  is_enabled?: boolean;
  is_preselected?: boolean;
}

export async function listRunnerModelCatalog(client: ApiClient): Promise<RunnerModelCatalogList> {
  const path = "/api/instances/runner-models/";
  const body = await client.getJson<unknown>(path);
  return client.parse(RunnerModelCatalogList, body, `GET ${path}`);
}

export async function createRunnerModel(client: ApiClient, write: RunnerModelWrite): Promise<RunnerCatalogRow> {
  const path = "/api/instances/runner-models/";
  const envelope = await client.sendJson<unknown>("POST", path, { json: { ...write } });
  return client.parse(RunnerCatalogRow, envelope.data, `POST ${path}`);
}

export async function updateRunnerModel(
  client: ApiClient,
  id: string,
  patch: RunnerModelWrite
): Promise<RunnerCatalogRow> {
  const path = `/api/instances/runner-models/${id}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { ...patch } });
  return client.parse(RunnerCatalogRow, envelope.data, `PATCH ${path}`);
}

export async function deleteRunnerModel(client: ApiClient, id: string): Promise<number> {
  const path = `/api/instances/runner-models/${id}/`;
  const response = await client.delete(path);
  return response.status;
}
