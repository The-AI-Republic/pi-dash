// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Instance-admin runners contract (ADM-080, from
// parity/specs/admin.md §11.2): the read-only, cross-workspace runner
// list. The endpoint answers {page, total, results} newest-first,
// fifty per page; the provisioning/status value sets ride the member
// runner contract (RUNNER_PROVISIONINGS/RUNNER_STATUSES) rather than
// being redeclared here.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { uuid } from "./fields.js";

/** One runner as the instance-admin list reports it: the flattened cross-workspace projection. */
export const InstanceRunner = z.object({
  id: uuid(),
  name: z.string(),
  workspace_slug: z.nullable(z.string()),
  owner_email: z.nullable(z.string()),
  host_label: z.string(),
  provisioning: z.string(),
  status: z.string(),
  runner_version: z.string(),
  codex_version: z.nullable(z.string()),
  last_heartbeat_at: z.nullable(z.string()),
  created_at: z.nullable(z.string()),
  revoked_at: z.nullable(z.string()),
});
export type InstanceRunner = z.infer<typeof InstanceRunner>;

/** Runners page: the page number, the total count, and the rows. */
export const InstanceRunnerListResponse = z.object({
  page: z.number(),
  total: z.number(),
  results: z.array(InstanceRunner),
});
export type InstanceRunnerListResponse = z.infer<typeof InstanceRunnerListResponse>;

export interface InstanceRunnerListQuery {
  provisioning?: string;
  status?: string;
  workspace?: string;
  page?: number;
}

/**
 * Lists every runner on the instance, newest-first. Blank filter
 * values are dropped from the URL: the backend rejects a
 * present-but-empty or unknown provisioning/status with 400, so the
 * unfiltered read must omit them rather than send them blank.
 */
export async function listInstanceRunners(
  client: ApiClient,
  query: InstanceRunnerListQuery = {}
): Promise<InstanceRunnerListResponse> {
  const path = "/api/instances/runners/";
  const body = await client.getJson<unknown>(path, {
    query: {
      provisioning: query.provisioning === undefined || query.provisioning === "" ? undefined : query.provisioning,
      status: query.status === undefined || query.status === "" ? undefined : query.status,
      workspace: query.workspace === undefined || query.workspace === "" ? undefined : query.workspace,
      page: query.page,
    },
  });
  return client.parse(InstanceRunnerListResponse, body, `GET ${path}`);
}
