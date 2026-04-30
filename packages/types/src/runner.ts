/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

export type TRunnerStatus = "online" | "offline" | "busy" | "revoked";

export interface IPodMini {
  id: string;
  name: string;
  is_default: boolean;
}

export interface IPod {
  id: string;
  name: string;
  description: string;
  is_default: boolean;
  workspace: string;
  created_by: string | null;
  runner_count: number;
  created_at: string;
  updated_at: string;
}

export interface IRunner {
  id: string;
  name: string;
  status: TRunnerStatus;
  os: string;
  arch: string;
  runner_version: string;
  protocol_version: number;
  capabilities: string[];
  last_heartbeat_at: string | null;
  owner: string | null;
  pod: string;
  pod_detail: IPodMini | null;
  /** Connection that owns this runner. Required post-refactor. */
  connection: string;
  created_at: string;
  updated_at: string;
}

export type TConnectionStatus = "pending" | "active" | "revoked";

export interface IConnection {
  id: string;
  name: string;
  host_label: string;
  status: TConnectionStatus;
  workspace: string;
  created_by: string | null;
  secret_fingerprint: string;
  enrolled_at: string | null;
  last_seen_at: string | null;
  created_at: string;
  revoked_at: string | null;
  runner_count: number;
}

/** ``POST /api/runners/connections/`` returns the row plus a one-time
 * enrollment token. ``enrollment_token`` is shown to the user exactly
 * once — there's no way to recover it after dismissal. */
export interface IConnectionWithToken extends IConnection {
  enrollment_token: string;
  enrollment_expires_at: string;
}

export type TAgentRunStatus =
  | "queued"
  | "assigned"
  | "running"
  | "awaiting_approval"
  | "awaiting_reauth"
  | "completed"
  | "failed"
  | "cancelled";

export const AGENT_RUN_TERMINAL_STATUSES: readonly TAgentRunStatus[] = ["completed", "failed", "cancelled"] as const;

export interface IAgentRunEvent {
  id: number;
  seq: number;
  kind: string;
  payload: Record<string, unknown>;
  created_at: string;
}

export interface IAgentRun {
  id: string;
  status: TAgentRunStatus;
  prompt: string;
  thread_id: string;
  runner: string | null;
  work_item: string | null;
  pod: string;
  pod_detail: IPodMini | null;
  /** Run creator — the access principal for permission checks. */
  created_by: string;
  /** Billable party — the runner's owner once assigned. NULL until assignment. */
  owner: string | null;
  created_at: string;
  assigned_at: string | null;
  started_at: string | null;
  ended_at: string | null;
  done_payload: Record<string, unknown> | null;
  error: string;
  events?: IAgentRunEvent[];
}

export type TApprovalKind = "command_execution" | "file_change" | "network_access" | "other";
export type TApprovalDecision = "accept" | "decline" | "accept_for_session";
export type TApprovalStatus = "pending" | "accepted" | "declined" | "expired";

export interface IApprovalRequest {
  id: string;
  agent_run: string;
  kind: TApprovalKind;
  payload: Record<string, unknown>;
  reason: string;
  status: TApprovalStatus;
  decision_source: string;
  requested_at: string;
  decided_at: string | null;
  expires_at: string | null;
}
