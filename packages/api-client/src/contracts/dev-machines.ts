// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Dev-machine contracts: the machines table with its rotate/revoke/delete
// actions, plus the cloud-driven runner creation that pushes a
// create-runner command down a machine's control session.
import * as z from "zod/mini";
import { ApiClient } from "../client.js";
import { dateTime, nullableDateTime, uuid } from "./fields.js";

/**
 * Why the server paused a machine's built-in engine on its own; "" when
 * it is not auto-paused. Kept an open string so a new server reason
 * still parses (and renders as-is).
 */
export const ENGINE_AUTO_PAUSE_REASONS = [
  "",
  "engine_sign_in_required",
  "gateway_session_revoked",
  "wallet_empty",
  "profile_unavailable",
  "engine_disabled",
] as const;
export type EngineAutoPauseReason = (typeof ENGINE_AUTO_PAUSE_REASONS)[number];

/**
 * The machine's engine sign-in state (design §4.3). Not served yet: the
 * second server change adds it, so clients degrade when it is absent.
 */
export const ENGINE_SIGN_IN_STATES = ["never", "active", "expiring", "expired", "signed_out", "revoked"] as const;
export type EngineSignInState = (typeof ENGINE_SIGN_IN_STATES)[number];

/**
 * Built-in engine controls carried on both machine shapes (design §8):
 * the owner's pause switch, the server's automatic pause reason and the
 * "Teammates can start runs on my issues" opt-in. All optional so
 * servers that predate the engine fields still parse.
 */
const engineFields = {
  engine_paused_by_owner: z.optional(z.boolean()),
  engine_auto_pause_reason: z.optional(z.string()),
  engine_team_runs: z.optional(z.boolean()),
};

/** Machine identity nested inside runner rows. */
export const DevMachineMini = z.object({
  id: uuid(),
  host_label: z.string(),
  label: z.string(),
  ...engineFields,
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
  /** `desktop_bundled` marks a legacy machine using desktop-session auth. */
  provisioning: z.optional(z.string()),
  runner_count: z.number(),
  online_runner_count: z.number(),
  control_online: z.boolean(),
  /**
   * Agent ids the daemon advertises (main#539). Absent from older servers
   * and empty for a daemon that predates the capability: both mean
   * "unknown", so clients must not gate anything on them.
   */
  supported_agents: z.optional(z.array(z.string())),
  ...engineFields,
  /** Engine sign-in state; absent until the server reports it. */
  engine_sign_in_state: z.optional(z.string()),
  /** When the engine credential expires; absent or null when unknown. */
  engine_credential_expires_at: z.optional(nullableDateTime()),
  /** Set on a machine merged into another one ("Replaced by"). */
  merged_into: z.optional(z.nullable(z.string())),
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

/**
 * Owner-only machine settings. `label` is the rename (served by the
 * second server change; older servers refuse it with 400).
 */
export interface DevMachinePatch {
  /** The owner's "Pause the built-in engine" switch. */
  engine_paused_by_owner?: boolean;
  /** "Teammates can start runs on my issues". */
  engine_team_runs?: boolean;
  /** User-chosen machine name; defaults to the hostname. */
  label?: string;
}

/**
 * Change a machine's engine controls or name. Owner only: anyone else
 * gets 403; a revoked machine answers 409. Answers the updated row.
 */
export async function updateDevMachine(
  client: ApiClient,
  machineId: string,
  workspaceId: string,
  patch: DevMachinePatch
): Promise<DevMachine> {
  const path = `/api/runners/dev-machines/${machineId}/`;
  const envelope = await client.sendJson<unknown>("PATCH", path, { json: { ...patch, workspace: workspaceId } });
  return client.parse(DevMachine, envelope.data, `PATCH ${path}`);
}

/**
 * Revoke the machine's engine credential only ("Sign out the engine",
 * design §4.3); the daemon and its own-agent runners stay signed in.
 * Served by the second server change; callers offer it only when the
 * machine reports its engine sign-in state. Answers the updated row.
 */
export async function signOutDevMachineEngine(
  client: ApiClient,
  machineId: string,
  workspaceId: string
): Promise<DevMachine> {
  const path = `/api/runners/dev-machines/${machineId}/engine-sign-out/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { workspace: workspaceId } });
  return client.parse(DevMachine, envelope.data, `POST ${path}`);
}

/**
 * The merge endpoint's answer (`machine_identity.merge_dev_machine`).
 */
export const MergeResult = z.object({
  source_dev_machine_id: uuid(),
  target_dev_machine_id: uuid(),
  merged_at: z.optional(z.string()),
  moved_runner_ids: z.array(z.string()),
  collisions: z.array(
    z.object({
      revoked_runner_id: z.string(),
      survivor_runner_id: z.string(),
    })
  ),
});
export type MergeResult = z.infer<typeof MergeResult>;

/**
 * Merge an offline machine into the computer that replaced it (design
 * §4.10, §13): its runners, pins and pod membership move to the target
 * and the source is revoked with `merged_into` set. Both machines must
 * be the caller's; the source must have no live session. Retrying into
 * the same target answers the same result.
 */
export async function mergeDevMachineInto(
  client: ApiClient,
  sourceMachineId: string,
  targetMachineId: string
): Promise<MergeResult> {
  const path = `/api/v1/runner/dev-machines/${sourceMachineId}/merge-into/`;
  const envelope = await client.sendJson<unknown>("POST", path, { json: { target: targetMachineId } });
  return client.parse(MergeResult, envelope.data, `POST ${path}`);
}

/**
 * One offline machine the app may offer to replace (design §4.10).
 * Ordered most recently seen first.
 */
export const ReplaceCandidate = z.object({
  id: uuid(),
  label: z.string(),
  host_label: z.string(),
  display_name: z.optional(z.nullable(z.string())),
  last_seen_at: nullableDateTime(),
  created_at: z.optional(nullableDateTime()),
  provisioning: z.optional(z.nullable(z.string())),
  live_runner_count: z.optional(z.number()),
});
export type ReplaceCandidate = z.infer<typeof ReplaceCandidate>;

export const ReplaceCandidates = z.object({
  candidates: z.array(ReplaceCandidate),
});
export type ReplaceCandidates = z.infer<typeof ReplaceCandidates>;

/**
 * The caller's offline machines with the same host label as `machineId`
 * and no live session: candidates for the "Is this <label>? Replace it"
 * prompt. The server never guesses; it only lists.
 */
export async function listReplaceCandidates(client: ApiClient, machineId: string): Promise<ReplaceCandidates> {
  const path = `/api/v1/runner/dev-machines/${machineId}/replace-candidates/`;
  const body = await client.getJson<unknown>(path);
  return client.parse(ReplaceCandidates, body, `GET ${path}`);
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
