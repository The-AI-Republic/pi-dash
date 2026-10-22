// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machine contracts: the machines table with its rotate/revoke/delete
// actions, plus the cloud-driven runner creation that pushes a
// create-runner command down a machine's control session.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";

/** Machine identity nested inside runner rows. */
export const DevMachineMini = z.object({
  id: uuid(),
  host_label: z.string(),
  label: z.string(),
});
export type DevMachineMini = z.infer<typeof DevMachineMini>;

/**
 * One dev-machine row. The counts and the heartbeat are annotated per
 * workspace; `control_online` tells whether the daemon can take a
 * cloud-pushed command right now.
 */
export const DevMachine = z.object({
  id: uuid(),
  host_label: z.string(),
  label: z.string(),
  visibility: z.number(),
  runner_count: z.number(),
  online_runner_count: z.number(),
  control_online: z.boolean(),
  /**
   * Agent ids the daemon advertises (main#539). Absent from older servers
   * and empty for a daemon that predates the capability: both mean
   * "unknown", so clients must not gate anything on them.
   */
  supported_agents: z.optional(z.array(z.string())),
  last_seen_at: nullableDateTime(),
  last_heartbeat_at: nullableDateTime(),
  revoked_at: nullableDateTime(),
  created_at: dateTime(),
  updated_at: dateTime(),
});
export type DevMachine = z.infer<typeof DevMachine>;

export const DevMachineList = z.array(DevMachine);
export type DevMachineList = z.infer<typeof DevMachineList>;

/**
 * Machines attached to a workspace: the caller's machines that host a
 * runner or hold a token there. Newest-seen first.
 */
export async function listDevMachines(client: ApiClient, workspaceId: string): Promise<DevMachineList> {
  const path = "/api/runners/dev-machines/";
  const body = await client.getJson<unknown>(path, { query: { workspace: workspaceId } });
  return client.parse(DevMachineList, body, `GET ${path}`);
}

/**
 * Invalidate a machine's tokens without touching the row; hosted
 * runners drop offline until the machine signs in again. Refused
 * with 409 on an already-revoked machine. Answers the updated row.
 */
export async function rotateDevMachine(client: ApiClient, machineId: string, workspaceId: string): Promise<DevMachine> {
  const path = `/api/runners/dev-machines/${machineId}/rotate/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { workspace: workspaceId } });
  return client.parse(DevMachine, envelope.data, `POST ${path}`);
}

/**
 * Revoke a machine for good: tokens die, hosted runners are revoked,
 * and the row stays behind showing the revoked state. Idempotent.
 * Answers the updated row.
 */
export async function revokeDevMachine(client: ApiClient, machineId: string, workspaceId: string): Promise<DevMachine> {
  const path = `/api/runners/dev-machines/${machineId}/revoke/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { workspace: workspaceId } });
  return client.parse(DevMachine, envelope.data, `POST ${path}`);
}

/**
 * Drop a machine connection entirely: tokens die, hosted runners are
 * torn down, and the row leaves the list. Cascades the teardown to
 * the daemon unless `purgeLocal` is false. Answers 204 with no body.
 */
export async function deleteDevMachine(
  client: ApiClient,
  machineId: string,
  workspaceId: string,
  options: { purgeLocal?: boolean } = {}
): Promise<number> {
  const purge = options.purgeLocal ?? true;
  const response = await client.delete(`/api/runners/dev-machines/${machineId}/`, {
    query: { workspace: workspaceId, purge_local: purge ? "true" : "false" },
  });
  return response.status;
}

export interface CreateRunnerOnMachineInput {
  /** Workspace id for scope; the machine must be attached to it. */
  workspace: string;
  /** Project identifier slug (not the uuid); the runner is created under it. */
  project: string;
  pod?: string;
  /** Blank auto-assigns; otherwise must match the runner-name charset. */
  name?: string;
  working_dir?: string;
  /** Agent CLI key; the server default applies when blank. */
  agent?: string;
  model?: string;
  reasoning_effort?: string;
}

/** Accepted creation: poll the request id for the daemon's verdict. */
export const CreateRunnerAccepted = z.object({
  request_id: uuid(),
});
export type CreateRunnerAccepted = z.infer<typeof CreateRunnerAccepted>;

/**
 * Ask a connected machine to create a runner. Answers 202 with the
 * request id; an offline machine answers 409 `machine_offline`.
 */
export async function createRunnerOnMachine(
  client: ApiClient,
  machineId: string,
  input: CreateRunnerOnMachineInput
): Promise<CreateRunnerAccepted> {
  const path = `/api/runners/dev-machines/${machineId}/create-runner/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { ...input } });
  return client.parse(CreateRunnerAccepted, envelope.data, `POST ${path}`);
}

export const CREATE_RUNNER_STATUSES = ["pending", "ok", "error"] as const;
export type CreateRunnerStatusValue = (typeof CREATE_RUNNER_STATUSES)[number];

/**
 * Daemon-reported outcome of a cloud-driven creation. A success
 * carries the new runner's id and name; a failure carries the reason.
 * Unknown request ids answer 404 instead of this shape.
 */
export const CreateRunnerStatus = z.object({
  request_id: uuid(),
  status: z.enum(CREATE_RUNNER_STATUSES),
  runner_id: z.optional(z.string()),
  runner_name: z.optional(z.string()),
  error: z.optional(z.string()),
});
export type CreateRunnerStatus = z.infer<typeof CreateRunnerStatus>;

/** Poll one creation request until it leaves `pending`. */
export async function getCreateRunnerStatus(
  client: ApiClient,
  machineId: string,
  requestId: string,
  workspaceId: string
): Promise<CreateRunnerStatus> {
  const path = `/api/runners/dev-machines/${machineId}/create-runner/${requestId}/`;
  const body = await client.getJson<unknown>(path, { query: { workspace: workspaceId } });
  return client.parse(CreateRunnerStatus, body, `GET ${path}`);
}
