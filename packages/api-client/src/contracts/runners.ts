// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner contracts: the overview list, the detail page, and the
// revoke/delete row actions. Live agent telemetry rides along on the
// detail shape; slices derive the activity badge from it client-side.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { DevMachineMini } from "./dev-machines.js";
import { dateTime, nullableDateTime, nullableUuid, uuid } from "./fields.js";
import { PodMini } from "./pods.js";

export const RUNNER_STATUSES = ["online", "offline", "busy", "revoked"] as const;
export type RunnerStatus = (typeof RUNNER_STATUSES)[number];

export const RUNNER_PROVISIONINGS = ["manual", "desktop_bundled"] as const;
export type RunnerProvisioning = (typeof RUNNER_PROVISIONINGS)[number];

/** Environment facts the runner reports when it opens a session. */
export const RunnerDevMetadata = z.object({
  working_dir: z.optional(z.string()),
});
export type RunnerDevMetadata = z.infer<typeof RunnerDevMetadata>;

/**
 * Volatile per-runner observability snapshot. Every reading is
 * nullable with null as unknown; the row itself is null until the
 * runner reports for the first time.
 */
export const RunnerLiveState = z.object({
  observed_run_id: nullableUuid(),
  last_event_at: nullableDateTime(),
  last_event_kind: z.nullable(z.string()),
  last_event_summary: z.nullable(z.string()),
  agent_pid: z.nullable(z.number()),
  agent_subprocess_alive: z.nullable(z.boolean()),
  approvals_pending: z.nullable(z.number()),
  input_tokens: z.nullable(z.number()),
  output_tokens: z.nullable(z.number()),
  total_tokens: z.nullable(z.number()),
  usage: z.record(z.string(), z.unknown()),
  llm_model: z.nullable(z.string()),
  turn_count: z.nullable(z.number()),
  updated_at: dateTime(),
});
export type RunnerLiveState = z.infer<typeof RunnerLiveState>;

/**
 * One runner row. `pod_detail` and the project behind it are always
 * present; `dev_machine_detail` is null for legacy enrollments, and
 * `enrolled_at` is null until the first token exchange.
 */
export const Runner = z.object({
  id: uuid(),
  name: z.string(),
  status: z.enum(RUNNER_STATUSES),
  host_label: z.string(),
  provisioning: z.enum(RUNNER_PROVISIONINGS),
  os: z.string(),
  arch: z.string(),
  runner_version: z.string(),
  dev_metadata: RunnerDevMetadata,
  protocol_version: z.number(),
  capabilities: z.array(z.string()),
  last_heartbeat_at: nullableDateTime(),
  owner: uuid(),
  dev_machine: nullableUuid(),
  dev_machine_detail: z.nullable(DevMachineMini),
  visibility: z.number(),
  pod: uuid(),
  pod_detail: PodMini,
  live_state: z.nullable(RunnerLiveState),
  enrolled_at: nullableDateTime(),
  revoked_at: nullableDateTime(),
  revoked_reason: z.string(),
  created_at: dateTime(),
  updated_at: dateTime(),
  /**
   * Hybrid-assignment answers, present only when the list was read with
   * `assignable_for` (design §7.4). Optional so plain list reads and
   * older backends still parse.
   */
  assignable: z.optional(z.boolean()),
  not_assignable_code: z.optional(z.string()),
  is_busy: z.optional(z.boolean()),
  queue_depth: z.optional(z.number()),
});
export type Runner = z.infer<typeof Runner>;

export const RunnerList = z.array(Runner);
export type RunnerList = z.infer<typeof RunnerList>;

export interface RunnerListScope {
  projectId?: string;
  podId?: string;
  /** Include desktop-bundled runners; they are hidden by default. */
  includeBundled?: boolean;
  /**
   * Hybrid-assignment eligibility read (design §7.4): an issue id, or
   * "new" for the create form. Each row then carries assignable,
   * not_assignable_code, is_busy and queue_depth.
   */
  assignableFor?: string;
}

/**
 * Runners in a workspace, optionally narrowed to a project or a
 * single pod. Private runners outside the caller's ownership stay
 * hidden. Most-recently-updated first.
 */
export async function listRunners(
  client: ApiClient,
  workspaceId: string,
  scope: RunnerListScope = {}
): Promise<RunnerList> {
  const path = "/api/runners/";
  const body = await client.getJson<unknown>(path, {
    query: {
      workspace: workspaceId,
      project: scope.projectId,
      pod: scope.podId,
      include_bundled: scope.includeBundled,
      assignable_for: scope.assignableFor,
    },
  });
  return client.parse(RunnerList, body, `GET ${path}`);
}

/** One runner row by id, with pod, machine, and live state nested. */
export async function getRunner(client: ApiClient, runnerId: string): Promise<Runner> {
  const path = `/api/runners/${runnerId}/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(Runner, body, `GET ${path}`);
}

/**
 * Revoke a runner for good: credentials die and in-flight runs are
 * settled, while the row stays for history. Idempotent. Answers
 * the updated row.
 */
export async function revokeRunner(client: ApiClient, runnerId: string): Promise<Runner> {
  const path = `/api/runners/${runnerId}/revoke/`;
  const envelope = await client.sendJson<unknown>("POST", path, {});
  return client.parse(Runner, envelope.data, `POST ${path}`);
}

/**
 * Drop a runner: in-flight runs are tombstoned as cancelled, the row
 * is removed, and historic runs survive with a cleared runner link.
 * Cascades the teardown to the daemon unless `purgeLocal` is false.
 * Answers 204 with no body.
 */
export async function deleteRunner(
  client: ApiClient,
  runnerId: string,
  options: { purgeLocal?: boolean } = {}
): Promise<number> {
  const purge = options.purgeLocal ?? true;
  const response = await client.delete(`/api/runners/${runnerId}/`, {
    query: { purge_local: purge ? "true" : "false" },
  });
  return response.status;
}
